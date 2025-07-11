use helium_store::{KVStore, MemStore, StateManager};

fn main() {
    // Create state manager and mount store
    let mut state_manager = StateManager::new_with_memstore();
    state_manager.mount_store("bank".to_string(), Box::new(MemStore::new()));

    // First, set data through get_store_mut
    {
        let store = state_manager.get_store_mut("bank").unwrap();
        store.set(b"balance_cosmos1test_stake", b"1000").unwrap();
        store.set(b"balance_cosmos1test_atom", b"500").unwrap();
    }

    println!("Initial data set directly on store");

    // Now test read-only access
    {
        let store = state_manager.get_store("bank").unwrap();
        let stake = store.get(b"balance_cosmos1test_stake").unwrap();
        match stake {
            Some(v) => println!("Read from store: {}", String::from_utf8_lossy(&v)),
            None => println!("Read from store: None"),
        }
    }

    // Commit the changes
    println!("Committing...");
    state_manager.commit().unwrap();

    // Now get mutable reference again (after commit)
    {
        let store = state_manager.get_store_mut("bank").unwrap();

        // Can we still read the data?
        let stake = store.get(b"balance_cosmos1test_stake").unwrap();
        match stake {
            Some(v) => println!(
                "Read from mutable store after commit: {}",
                String::from_utf8_lossy(&v)
            ),
            None => println!("Read from mutable store after commit: None"),
        }

        // Add more data
        store.set(b"balance_cosmos1other_stake", b"2000").unwrap();
    }

    // Now try to read again (store is in cache now)
    {
        let store = state_manager.get_store("bank").unwrap();
        let stake = store.get(b"balance_cosmos1test_stake").unwrap();
        match stake {
            Some(v) => println!(
                "Read from store after cache: {}",
                String::from_utf8_lossy(&v)
            ),
            None => println!("Read from store after cache: None"),
        }
    }
}
