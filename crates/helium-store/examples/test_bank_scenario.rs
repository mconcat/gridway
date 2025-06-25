use helium_store::{KVStore, MemStore, StateManager};

fn main() {
    println!("Testing bank service scenario with StateManager...\n");
    
    // Create state manager and mount bank store
    let mut state_manager = StateManager::new();
    state_manager.mount_store("bank".to_string(), Box::new(MemStore::new()));
    
    // Simulate bank service setting balances
    println!("1. Setting initial balances...");
    {
        let store = state_manager.get_store_mut("bank").unwrap();
        store.set(b"balance_cosmos1test_stake".to_vec(), b"1000".to_vec()).unwrap();
        store.set(b"balance_cosmos1test_atom".to_vec(), b"500".to_vec()).unwrap();
        store.set(b"balance_cosmos1other_stake".to_vec(), b"2000".to_vec()).unwrap();
        store.set(b"balance_cosmos1other_atom".to_vec(), b"750".to_vec()).unwrap();
    }
    
    // Commit the changes
    println!("2. Committing changes...");
    state_manager.commit().unwrap();
    
    // Simulate bank service querying all balances for an address
    println!("\n3. Querying all balances for cosmos1test using prefix iterator...");
    {
        let store = state_manager.get_store("bank").unwrap();
        let prefix = b"balance_cosmos1test_";
        let results: Vec<_> = store.prefix_iterator(prefix).collect();
        
        println!("   Found {} balances for cosmos1test:", results.len());
        for (key, value) in &results {
            let key_str = String::from_utf8_lossy(key);
            let value_str = String::from_utf8_lossy(value);
            println!("   - {}: {}", key_str, value_str);
        }
    }
    
    // Simulate transfer (update balances)
    println!("\n4. Simulating transfer: cosmos1test sends 100 stake to cosmos1other...");
    {
        let store = state_manager.get_store_mut("bank").unwrap();
        
        // Read current balances
        let test_stake = store.get(b"balance_cosmos1test_stake").unwrap()
            .map(|v| String::from_utf8_lossy(&v).parse::<u64>().unwrap())
            .unwrap_or(0);
        let other_stake = store.get(b"balance_cosmos1other_stake").unwrap()
            .map(|v| String::from_utf8_lossy(&v).parse::<u64>().unwrap())
            .unwrap_or(0);
        
        println!("   Current balances: test={}, other={}", test_stake, other_stake);
        
        // Update balances
        let new_test_stake = test_stake - 100;
        let new_other_stake = other_stake + 100;
        
        store.set(b"balance_cosmos1test_stake".to_vec(), new_test_stake.to_string().into_bytes()).unwrap();
        store.set(b"balance_cosmos1other_stake".to_vec(), new_other_stake.to_string().into_bytes()).unwrap();
        
        println!("   New balances: test={}, other={}", new_test_stake, new_other_stake);
    }
    
    // Check balances before commit (should see updated values)
    println!("\n5. Checking balances before commit...");
    {
        let store = state_manager.get_store("bank").unwrap();
        let test_stake = store.get(b"balance_cosmos1test_stake").unwrap()
            .map(|v| String::from_utf8_lossy(&v).to_string())
            .unwrap_or("0".to_string());
        println!("   cosmos1test stake balance: {}", test_stake);
    }
    
    // Commit the transfer
    println!("\n6. Committing transfer...");
    state_manager.commit().unwrap();
    
    // Final balance check
    println!("\n7. Final balance check for all addresses...");
    {
        let store = state_manager.get_store("bank").unwrap();
        
        // Check cosmos1test balances
        println!("   cosmos1test balances:");
        let prefix = b"balance_cosmos1test_";
        let results: Vec<_> = store.prefix_iterator(prefix).collect();
        for (key, value) in &results {
            let key_str = String::from_utf8_lossy(key);
            let value_str = String::from_utf8_lossy(value);
            println!("     - {}: {}", key_str, value_str);
        }
        
        // Check cosmos1other balances
        println!("   cosmos1other balances:");
        let prefix = b"balance_cosmos1other_";
        let results: Vec<_> = store.prefix_iterator(prefix).collect();
        for (key, value) in &results {
            let key_str = String::from_utf8_lossy(key);
            let value_str = String::from_utf8_lossy(value);
            println!("     - {}: {}", key_str, value_str);
        }
    }
    
    println!("\nTest completed successfully!");
}