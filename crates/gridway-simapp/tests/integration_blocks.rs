//! Multi-block integration tests for gridway.
//!
//! These tests verify higher-level behaviors that span multiple blocks:
//!   a) Multi-block transfer chain — A→B→C→D across 4 blocks
//!   b) Empty block handling — state root stability
//!   c) Large block — 10 transfers in one block
//!   d) Replay consistency — deterministic replay on fresh BaseApp

use gridway_simapp::{
    build_transfer_tx, setup_genesis, SimState, TEST_CHAIN_ID, TEST_DENOM,
};

// ─── (a) Multi-block Transfer Chain ──────────────────────────────────────────
//
// A→B (block 1), B→C (block 2), C→D (block 3), D→A (block 4).
// Final: each account should have the initial balance.

#[test]
fn multi_block_transfer_chain() {
    let initial = 100_000u64;
    let transfer_amount = 25_000u64;
    let n = 4;

    let (mut app, accounts, mut sim) = setup_genesis(n, initial);

    // Block 1: A → B
    {
        let (ref key, ref from) = accounts[0];
        let (_, ref to) = accounts[1];
        let seq = sim.get_sequence(from);
        let tx = build_transfer_tx(key, to, transfer_amount, TEST_DENOM, seq);
        let (_root, responses) = app.execute_block(1, 1000, TEST_CHAIN_ID, &[tx]).unwrap();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].code, 0, "A→B failed: {}", responses[0].log);
        app.commit().unwrap();
        sim.apply_transfer(from, to, TEST_DENOM, transfer_amount);
        sim.increment_sequence(from);
    }

    // Block 2: B → C
    {
        let (ref key, ref from) = accounts[1];
        let (_, ref to) = accounts[2];
        let seq = sim.get_sequence(from);
        let tx = build_transfer_tx(key, to, transfer_amount, TEST_DENOM, seq);
        let (_root, responses) = app.execute_block(2, 2000, TEST_CHAIN_ID, &[tx]).unwrap();
        assert_eq!(responses[0].code, 0, "B→C failed: {}", responses[0].log);
        app.commit().unwrap();
        sim.apply_transfer(from, to, TEST_DENOM, transfer_amount);
        sim.increment_sequence(from);
    }

    // Block 3: C → D
    {
        let (ref key, ref from) = accounts[2];
        let (_, ref to) = accounts[3];
        let seq = sim.get_sequence(from);
        let tx = build_transfer_tx(key, to, transfer_amount, TEST_DENOM, seq);
        let (_root, responses) = app.execute_block(3, 3000, TEST_CHAIN_ID, &[tx]).unwrap();
        assert_eq!(responses[0].code, 0, "C→D failed: {}", responses[0].log);
        app.commit().unwrap();
        sim.apply_transfer(from, to, TEST_DENOM, transfer_amount);
        sim.increment_sequence(from);
    }

    // Block 4: D → A
    {
        let (ref key, ref from) = accounts[3];
        let (_, ref to) = accounts[0];
        let seq = sim.get_sequence(from);
        let tx = build_transfer_tx(key, to, transfer_amount, TEST_DENOM, seq);
        let (_root, responses) = app.execute_block(4, 4000, TEST_CHAIN_ID, &[tx]).unwrap();
        assert_eq!(responses[0].code, 0, "D→A failed: {}", responses[0].log);
        app.commit().unwrap();
        sim.apply_transfer(from, to, TEST_DENOM, transfer_amount);
        sim.increment_sequence(from);
    }

    // Verify: all balances back to initial (cyclic transfer)
    for (_, addr) in &accounts {
        let bal = app.get_balance(addr, TEST_DENOM).unwrap();
        assert_eq!(bal, initial,
            "account {} should have {} after cyclic transfers, got {}", addr, initial, bal);
    }

    // SimState should also agree
    sim.verify_balances(&app).expect("SimState should match on-chain after chain transfer");

    // Total supply conserved
    let total: u64 = accounts.iter()
        .map(|(_, addr)| app.get_balance(addr, TEST_DENOM).unwrap())
        .sum();
    assert_eq!(total, initial * n as u64);
}

// ─── (b) Empty Block Handling ────────────────────────────────────────────────
//
// Executing 10 empty blocks should not change the state root.

#[test]
fn empty_block_handling() {
    let (mut app, _accounts, _sim) = setup_genesis(2, 50_000);

    // Execute first block to establish post-genesis baseline
    // (hooks may write block metadata, changing root from genesis)
    let (first_root, responses) = app
        .execute_block(1, 1000, TEST_CHAIN_ID, &[])
        .expect("empty block should succeed");
    app.commit().unwrap();
    assert!(responses.is_empty(), "empty block should produce no responses");

    let mut prev_root = first_root;
    for height in 2..=10 {
        let (root, responses) = app
            .execute_block(height, height * 1000, TEST_CHAIN_ID, &[])
            .expect("empty block should succeed");
        app.commit().unwrap();

        assert!(responses.is_empty(), "empty block should produce no responses");

        // State root should remain constant across empty blocks
        // (after the first block establishes the post-genesis baseline)
        assert_eq!(root, prev_root,
            "state root should be stable across empty blocks (block {}): {:?} vs {:?}",
            height, hex::encode(root), hex::encode(prev_root));
        prev_root = root;
    }
}

// ─── (c) Large Block ─────────────────────────────────────────────────────────
//
// 10 transfers from a single sender in one block.
// Verify final balances and that all TxResponses succeed.

#[test]
fn large_block() {
    let n_transfers = 10usize;
    let initial = 10_000_000u64; // enough for transfers
    let transfer_amount = 1_000u64;

    let (mut app, accounts, mut sim) = setup_genesis(2, initial);

    let (ref from_key, ref from_addr) = accounts[0];
    let (_, ref to_addr) = accounts[1];

    // Build transactions with increasing sequence numbers
    let mut txs = Vec::with_capacity(n_transfers);
    for i in 0..n_transfers {
        let tx = build_transfer_tx(from_key, to_addr, transfer_amount, TEST_DENOM, i as u64);
        txs.push(tx);
    }

    // Execute all in one block
    let (_root, responses) = app
        .execute_block(1, 1000, TEST_CHAIN_ID, &txs)
        .expect("large block should succeed");
    app.commit().unwrap();

    assert_eq!(responses.len(), n_transfers);

    let success_count = responses.iter().filter(|r| r.code == 0).count();
    assert_eq!(success_count, n_transfers,
        "all {} transfers should succeed, got {} successes (first failure: {:?})",
        n_transfers, success_count,
        responses.iter().find(|r| r.code != 0).map(|r| &r.log));

    // Update SimState
    for _ in 0..n_transfers {
        sim.apply_transfer(from_addr, to_addr, TEST_DENOM, transfer_amount);
        sim.increment_sequence(from_addr);
    }

    // Verify balances
    let expected_from = initial - (transfer_amount * n_transfers as u64);
    let expected_to = initial + (transfer_amount * n_transfers as u64);

    let from_bal = app.get_balance(from_addr, TEST_DENOM).unwrap();
    let to_bal = app.get_balance(to_addr, TEST_DENOM).unwrap();

    assert_eq!(from_bal, expected_from,
        "sender should have {}, got {}", expected_from, from_bal);
    assert_eq!(to_bal, expected_to,
        "receiver should have {}, got {}", expected_to, to_bal);

    // Total conserved
    assert_eq!(from_bal + to_bal, initial * 2);

    sim.verify_balances(&app).expect("SimState should match after large block");
}

// ─── (d) Replay Consistency ──────────────────────────────────────────────────
//
// Execute a sequence of blocks, record the state roots, then replay the
// exact same sequence on a fresh BaseApp and verify identical roots.

#[test]
fn replay_consistency() {
    let initial = 500_000u64;
    let n_accounts = 3;

    // --- First run: execute and record ---

    let (mut app1, accounts1, _sim1) = setup_genesis(n_accounts, initial);
    let genesis_root = app1.commit().unwrap();

    // We'll build the same raw tx bytes for both runs
    // Since keys are deterministic from seeds, we can reproduce them

    // Block 1: account[0] → account[1], 10000
    let tx1 = build_transfer_tx(&accounts1[0].0, &accounts1[1].1, 10_000, TEST_DENOM, 0);
    let (root1, resp1) = app1.execute_block(1, 1000, TEST_CHAIN_ID, &[tx1.clone()]).unwrap();
    assert_eq!(resp1[0].code, 0, "run1 block1: {}", resp1[0].log);
    app1.commit().unwrap();

    // Block 2: account[1] → account[2], 5000
    let tx2 = build_transfer_tx(&accounts1[1].0, &accounts1[2].1, 5_000, TEST_DENOM, 0);
    let (root2, resp2) = app1.execute_block(2, 2000, TEST_CHAIN_ID, &[tx2.clone()]).unwrap();
    assert_eq!(resp2[0].code, 0, "run1 block2: {}", resp2[0].log);
    app1.commit().unwrap();

    // Block 3: empty
    let (root3, _) = app1.execute_block(3, 3000, TEST_CHAIN_ID, &[]).unwrap();
    app1.commit().unwrap();

    // Block 4: account[2] → account[0], 2000
    let tx4 = build_transfer_tx(&accounts1[2].0, &accounts1[0].1, 2_000, TEST_DENOM, 0);
    let (root4, resp4) = app1.execute_block(4, 4000, TEST_CHAIN_ID, &[tx4.clone()]).unwrap();
    assert_eq!(resp4[0].code, 0, "run1 block4: {}", resp4[0].log);
    app1.commit().unwrap();

    // --- Second run: replay with fresh BaseApp ---

    let (mut app2, _accounts2, _sim2) = setup_genesis(n_accounts, initial);
    let genesis_root2 = app2.commit().unwrap();
    assert_eq!(genesis_root, genesis_root2, "genesis roots must match");

    let (replay_root1, replay_resp1) = app2.execute_block(1, 1000, TEST_CHAIN_ID, &[tx1]).unwrap();
    assert_eq!(replay_resp1[0].code, 0, "replay block1: {}", replay_resp1[0].log);
    app2.commit().unwrap();
    assert_eq!(root1, replay_root1, "block 1 roots diverged");

    let (replay_root2, replay_resp2) = app2.execute_block(2, 2000, TEST_CHAIN_ID, &[tx2]).unwrap();
    assert_eq!(replay_resp2[0].code, 0, "replay block2: {}", replay_resp2[0].log);
    app2.commit().unwrap();
    assert_eq!(root2, replay_root2, "block 2 roots diverged");

    let (replay_root3, _) = app2.execute_block(3, 3000, TEST_CHAIN_ID, &[]).unwrap();
    app2.commit().unwrap();
    assert_eq!(root3, replay_root3, "block 3 roots diverged");

    let (replay_root4, replay_resp4) = app2.execute_block(4, 4000, TEST_CHAIN_ID, &[tx4]).unwrap();
    assert_eq!(replay_resp4[0].code, 0, "replay block4: {}", replay_resp4[0].log);
    app2.commit().unwrap();
    assert_eq!(root4, replay_root4, "block 4 roots diverged");

    // Final balances must match
    for i in 0..n_accounts {
        let (_, ref addr) = _accounts2[i];
        let bal1 = app1.get_balance(addr, TEST_DENOM).unwrap();
        let bal2 = app2.get_balance(addr, TEST_DENOM).unwrap();
        assert_eq!(bal1, bal2,
            "balance mismatch for account {} after replay: {} vs {}", addr, bal1, bal2);
    }
}
