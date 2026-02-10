//! Property-based tests for gridway bank module using proptest.
//!
//! Tests fundamental invariants that must hold for any sequence of
//! bank transfers:
//!   a) Balance conservation — total supply never changes
//!   b) Transfer correctness — sender loses X, receiver gains X
//!   c) Overdraft rejection — transfers > balance fail cleanly
//!   d) State root determinism — same inputs → same state root
//!   e) Mempool invariants — len/size/dedup correct after N ops

use gridway_baseapp::BaseApp;
use gridway_consensus::mempool::{Mempool, MempoolConfig};
use gridway_simapp::{
    build_transfer_tx, setup_genesis, SimState, TEST_CHAIN_ID, TEST_DENOM,
};

use proptest::prelude::*;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Execute a block containing a single transfer and return whether it succeeded.
/// Updates the SimState if the transfer succeeds.
fn try_transfer(
    app: &mut BaseApp,
    sim: &mut SimState,
    from_key: &commonware_cryptography::ed25519::PrivateKey,
    from_addr: &str,
    to_addr: &str,
    amount: u64,
    height: u64,
) -> bool {
    let seq = sim.get_sequence(from_addr);
    let tx = build_transfer_tx(from_key, to_addr, amount, TEST_DENOM, seq);

    let (_root, responses) = app
        .execute_block(height, height * 1000, TEST_CHAIN_ID, &[tx])
        .expect("execute_block should not panic");
    app.commit().expect("commit should succeed");

    if responses.len() == 1 && responses[0].code == 0 {
        sim.apply_transfer(from_addr, to_addr, TEST_DENOM, amount);
        sim.increment_sequence(from_addr);
        true
    } else {
        false
    }
}

// ─── proptest config ─────────────────────────────────────────────────────────

fn test_config() -> ProptestConfig {
    ProptestConfig {
        cases: 8, // reduced from 64: WASM execution in debug mode is very slow
        max_shrink_iters: 256,
        ..ProptestConfig::default()
    }
}

// ─── (a) Balance Conservation ────────────────────────────────────────────────
//
// For any random sequence of transfers among N accounts, the total supply
// across all accounts must remain constant.

proptest! {
    #![proptest_config(test_config())]

    #[test]
    fn balance_conservation(
        // Generate 2–6 transfers, each: (sender_idx, receiver_idx, amount)
        transfers in prop::collection::vec(
            (0usize..4, 0usize..4, 1u64..5_000),
            2..=6,
        )
    ) {
        let n_accounts = 4;
        let initial_balance = 100_000u64;
        let (mut app, accounts, mut sim) = setup_genesis(n_accounts, initial_balance);

        let expected_total = initial_balance * n_accounts as u64;

        for (i, (from_idx, to_idx, amount)) in transfers.iter().enumerate() {
            let from_idx = *from_idx;
            let to_idx = if *to_idx == from_idx {
                (from_idx + 1) % n_accounts
            } else {
                *to_idx
            };

            let (ref from_key, ref from_addr) = accounts[from_idx];
            let (_, ref to_addr) = accounts[to_idx];

            // Only transfer if the sender has enough
            let sender_bal = sim.get_balance(from_addr, TEST_DENOM);
            let xfer_amount = (*amount).min(sender_bal);
            if xfer_amount == 0 {
                continue;
            }

            let height = (i + 1) as u64;
            try_transfer(&mut app, &mut sim, from_key, from_addr, to_addr, xfer_amount, height);
        }

        // INVARIANT: total supply unchanged
        let mut actual_total = 0u64;
        for (_, addr) in &accounts {
            actual_total += app.get_balance(addr, TEST_DENOM).unwrap();
        }
        prop_assert_eq!(actual_total, expected_total,
            "total supply must be conserved: expected {}, got {}", expected_total, actual_total);

        // Also verify SimState matches on-chain
        sim.verify_balances(&app).map_err(|e| {
            TestCaseError::Fail(format!("SimState/on-chain mismatch: {}", e).into())
        })?;
    }
}

// ─── (b) Transfer Correctness ────────────────────────────────────────────────
//
// A single valid transfer: sender balance decreases by exactly X,
// receiver balance increases by exactly X.

proptest! {
    #![proptest_config(test_config())]

    #[test]
    fn transfer_correctness(amount in 1u64..50_000) {
        let initial = 100_000u64;
        let (mut app, accounts, mut sim) = setup_genesis(2, initial);

        let (ref from_key, ref from_addr) = accounts[0];
        let (_, ref to_addr) = accounts[1];

        let success = try_transfer(&mut app, &mut sim, from_key, from_addr, to_addr, amount, 1);
        prop_assert!(success, "transfer of {} should succeed (sender has {})", amount, initial);

        let from_bal = app.get_balance(from_addr, TEST_DENOM).unwrap();
        let to_bal = app.get_balance(to_addr, TEST_DENOM).unwrap();

        prop_assert_eq!(from_bal, initial - amount,
            "sender should have {} - {} = {}, got {}", initial, amount, initial - amount, from_bal);
        prop_assert_eq!(to_bal, initial + amount,
            "receiver should have {} + {} = {}, got {}", initial, amount, initial + amount, to_bal);
    }
}

// ─── (c) Overdraft Rejection ─────────────────────────────────────────────────
//
// A transfer exceeding the sender's balance must fail, and both balances
// must remain unchanged.

proptest! {
    #![proptest_config(test_config())]

    #[test]
    fn overdraft_rejection(
        initial in 100u64..10_000,
        extra in 1u64..50_000,
    ) {
        let overdraft_amount = initial + extra; // guaranteed > initial

        let (mut app, accounts, mut sim) = setup_genesis(2, initial);

        let (ref from_key, ref from_addr) = accounts[0];
        let (_, ref to_addr) = accounts[1];

        let from_before = app.get_balance(from_addr, TEST_DENOM).unwrap();
        let to_before = app.get_balance(to_addr, TEST_DENOM).unwrap();

        let success = try_transfer(
            &mut app, &mut sim, from_key, from_addr, to_addr, overdraft_amount, 1,
        );
        prop_assert!(!success,
            "transfer of {} should fail (sender only has {})", overdraft_amount, initial);

        // Balances must be unchanged
        let from_after = app.get_balance(from_addr, TEST_DENOM).unwrap();
        let to_after = app.get_balance(to_addr, TEST_DENOM).unwrap();

        prop_assert_eq!(from_after, from_before,
            "sender balance should be unchanged after overdraft");
        prop_assert_eq!(to_after, to_before,
            "receiver balance should be unchanged after overdraft");
    }
}

// ─── (d) State Root Determinism ──────────────────────────────────────────────
//
// Executing the exact same transaction sequence on two independently
// constructed BaseApps must produce identical state roots.

proptest! {
    #![proptest_config(test_config())]

    #[test]
    fn state_root_determinism(
        amounts in prop::collection::vec(1u64..10_000, 1..=4),
    ) {
        let initial = 1_000_000u64;

        let run = |amounts: &[u64]| -> Vec<[u8; 32]> {
            let (mut app, accounts, _sim) = setup_genesis(2, initial);
            let mut roots = Vec::new();

            for (i, &amount) in amounts.iter().enumerate() {
                let (ref key, ref from) = accounts[0];
                let (_, ref to) = accounts[1];
                let seq = i as u64;
                let tx = build_transfer_tx(key, to, amount, TEST_DENOM, seq);
                let (root, _) = app
                    .execute_block((i + 1) as u64, (i + 1) as u64 * 1000, TEST_CHAIN_ID, &[tx])
                    .expect("execute_block");
                app.commit().expect("commit");
                roots.push(root);
            }
            roots
        };

        let roots_a = run(&amounts);
        let roots_b = run(&amounts);

        for (i, (a, b)) in roots_a.iter().zip(roots_b.iter()).enumerate() {
            prop_assert_eq!(a, b,
                "state root diverged at block {}: {:?} vs {:?}", i + 1, hex::encode(a), hex::encode(b));
        }
    }
}

// ─── (e) Mempool Invariants ──────────────────────────────────────────────────
//
// After N submits and M drains, the mempool's len, total_size, and dedup
// set must be internally consistent.

proptest! {
    #![proptest_config(test_config())]

    #[test]
    fn mempool_invariants(
        // Each op: true = submit a unique tx of given size, false = drain up to `size` txs
        ops in prop::collection::vec(
            (any::<bool>(), 1usize..64),
            4..=20,
        )
    ) {
        let config = MempoolConfig {
            max_txs: 100,
            max_tx_size: 1024,
            max_total_size: 8192,
        };
        let mut pool = Mempool::new(config);

        // Track expected state
        let mut expected_txs: Vec<Vec<u8>> = Vec::new();
        let mut next_id: u32 = 0;

        for (is_submit, param) in &ops {
            if *is_submit {
                // Submit a unique tx of `param` bytes (capped at max_tx_size)
                let size = (*param).min(1024);
                let mut tx = vec![0u8; size];
                // Make unique by embedding a counter
                let id_bytes = next_id.to_le_bytes();
                for (i, b) in id_bytes.iter().enumerate() {
                    if i < size {
                        tx[i] = *b;
                    }
                }
                next_id += 1;

                match pool.submit(tx.clone()) {
                    Ok(_) => expected_txs.push(tx),
                    Err(_) => { /* full or too large — skip */ }
                }
            } else {
                // Drain up to `param` txs
                let drain_count = *param;
                let drained = pool.drain(drain_count);
                let actual_drain = drained.len().min(expected_txs.len());
                expected_txs.drain(..actual_drain);
            }

            // INVARIANT: len matches
            prop_assert_eq!(pool.len(), expected_txs.len(),
                "mempool len mismatch: pool={}, expected={}", pool.len(), expected_txs.len());

            // INVARIANT: total_size matches sum of expected tx sizes
            let expected_size: usize = expected_txs.iter().map(|t| t.len()).sum();
            prop_assert_eq!(pool.total_size(), expected_size,
                "total_size mismatch: pool={}, expected={}", pool.total_size(), expected_size);
        }

        // INVARIANT: duplicate detection — re-submitting an existing tx fails
        if let Some(existing) = expected_txs.first().cloned() {
            let result = pool.submit(existing);
            prop_assert!(result.is_err(), "re-submitting existing tx should fail as duplicate");
        }
    }
}
