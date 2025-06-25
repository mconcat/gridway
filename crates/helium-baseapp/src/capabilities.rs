//! Module Capability System
//!
//! This module implements a capability-based security model for WASM modules in the microkernel architecture.
//! Modules must declare their required capabilities at registration time and can only access resources
//! for which they have been granted capabilities. This provides fine-grained security isolation between modules.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use thiserror::Error;
use tracing::{debug, info};

/// Capability system errors
#[derive(Error, Debug)]
pub enum CapabilityError {
    /// Capability not granted to module
    #[error("capability not granted: {0}")]
    NotGranted(String),
    
    /// Invalid capability format
    #[error("invalid capability format: {0}")]
    InvalidFormat(String),
    
    /// Capability already exists
    #[error("capability already exists: {0}")]
    AlreadyExists(String),
    
    /// Module not found
    #[error("module not found: {0}")]
    ModuleNotFound(String),
    
    /// Invalid capability delegation
    #[error("invalid delegation: {0}")]
    InvalidDelegation(String),
    
    /// Circular dependency detected
    #[error("circular dependency detected in capability chain")]
    CircularDependency,
    
    /// Lock poisoned
    #[error("lock poisoned: {0}")]
    LockPoisoned(String),
}

pub type Result<T> = std::result::Result<T, CapabilityError>;

/// Types of capabilities that can be granted to modules
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CapabilityType {
    /// Read state from a specific namespace
    ReadState(String),
    
    /// Write state to a specific namespace
    WriteState(String),
    
    /// Delete state from a specific namespace
    DeleteState(String),
    
    /// List keys in a specific namespace
    ListState(String),
    
    /// Send messages of a specific type
    SendMessage(String),
    
    /// Receive messages of a specific type
    ReceiveMessage(String),
    
    /// Emit events of a specific type
    EmitEvent(String),
    
    /// Allocate memory (with size limit)
    AllocateMemory(u64),
    
    /// Access system information
    SystemInfo,
    
    /// Execute other modules (with module name)
    ExecuteModule(String),
    
    /// Create new capabilities (admin capability)
    CreateCapability,
    
    /// Delegate capabilities to other modules
    DelegateCapability,
    
    /// Access network resources
    Network(NetworkCapability),
    
    /// Access cryptographic functions
    Crypto(CryptoCapability),
    
    /// Custom capability for extensions
    Custom(String, String), // (namespace, operation)
}

/// Network-related capabilities
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NetworkCapability {
    /// Make HTTP requests to specific domains
    HttpRequest(String),
    
    /// Open TCP connections
    TcpConnect(String, u16), // (host, port)
    
    /// Listen on specific ports
    TcpListen(u16),
}

/// Cryptographic capabilities
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CryptoCapability {
    /// Generate cryptographic keys
    GenerateKeys,
    
    /// Sign data
    Sign,
    
    /// Verify signatures
    Verify,
    
    /// Hash data
    Hash,
    
    /// Encrypt/decrypt data
    Encrypt,
}

impl CapabilityType {
    /// Parse a capability from string format
    /// Format: "operation:target" or "operation:target:subtarget"
    pub fn from_string(cap_str: &str) -> Result<Self> {
        let parts: Vec<&str> = cap_str.split(':').collect();
        
        if parts.is_empty() {
            return Err(CapabilityError::InvalidFormat("empty capability string".to_string()));
        }
        
        match parts[0] {
            "read_state" => {
                if parts.len() != 2 {
                    return Err(CapabilityError::InvalidFormat("read_state requires namespace".to_string()));
                }
                Ok(CapabilityType::ReadState(parts[1].to_string()))
            }
            "write_state" => {
                if parts.len() != 2 {
                    return Err(CapabilityError::InvalidFormat("write_state requires namespace".to_string()));
                }
                Ok(CapabilityType::WriteState(parts[1].to_string()))
            }
            "delete_state" => {
                if parts.len() != 2 {
                    return Err(CapabilityError::InvalidFormat("delete_state requires namespace".to_string()));
                }
                Ok(CapabilityType::DeleteState(parts[1].to_string()))
            }
            "list_state" => {
                if parts.len() != 2 {
                    return Err(CapabilityError::InvalidFormat("list_state requires namespace".to_string()));
                }
                Ok(CapabilityType::ListState(parts[1].to_string()))
            }
            "send_msg" => {
                if parts.len() != 2 {
                    return Err(CapabilityError::InvalidFormat("send_msg requires message type".to_string()));
                }
                Ok(CapabilityType::SendMessage(parts[1].to_string()))
            }
            "receive_msg" => {
                if parts.len() != 2 {
                    return Err(CapabilityError::InvalidFormat("receive_msg requires message type".to_string()));
                }
                Ok(CapabilityType::ReceiveMessage(parts[1].to_string()))
            }
            "emit_event" => {
                if parts.len() != 2 {
                    return Err(CapabilityError::InvalidFormat("emit_event requires event type".to_string()));
                }
                Ok(CapabilityType::EmitEvent(parts[1].to_string()))
            }
            "execute_module" => {
                if parts.len() != 2 {
                    return Err(CapabilityError::InvalidFormat("execute_module requires module name".to_string()));
                }
                Ok(CapabilityType::ExecuteModule(parts[1].to_string()))
            }
            "allocate_memory" => {
                if parts.len() != 2 {
                    return Err(CapabilityError::InvalidFormat("allocate_memory requires size limit".to_string()));
                }
                let size = parts[1].parse::<u64>()
                    .map_err(|_| CapabilityError::InvalidFormat("invalid memory size".to_string()))?;
                Ok(CapabilityType::AllocateMemory(size))
            }
            "system_info" => Ok(CapabilityType::SystemInfo),
            "create_capability" => Ok(CapabilityType::CreateCapability),
            "delegate_capability" => Ok(CapabilityType::DelegateCapability),
            "crypto" => {
                if parts.len() != 2 {
                    return Err(CapabilityError::InvalidFormat("crypto requires operation".to_string()));
                }
                let crypto_cap = match parts[1] {
                    "generate_keys" => CryptoCapability::GenerateKeys,
                    "sign" => CryptoCapability::Sign,
                    "verify" => CryptoCapability::Verify,
                    "hash" => CryptoCapability::Hash,
                    "encrypt" => CryptoCapability::Encrypt,
                    _ => return Err(CapabilityError::InvalidFormat(format!("unknown crypto operation: {}", parts[1]))),
                };
                Ok(CapabilityType::Crypto(crypto_cap))
            }
            "network" => {
                if parts.len() < 2 {
                    return Err(CapabilityError::InvalidFormat("network requires operation".to_string()));
                }
                let net_cap = match parts[1] {
                    "http" => {
                        if parts.len() != 3 {
                            return Err(CapabilityError::InvalidFormat("network:http requires domain".to_string()));
                        }
                        NetworkCapability::HttpRequest(parts[2].to_string())
                    }
                    "tcp_connect" => {
                        if parts.len() != 4 {
                            return Err(CapabilityError::InvalidFormat("network:tcp_connect requires host:port".to_string()));
                        }
                        let port = parts[3].parse::<u16>()
                            .map_err(|_| CapabilityError::InvalidFormat("invalid port number".to_string()))?;
                        NetworkCapability::TcpConnect(parts[2].to_string(), port)
                    }
                    "tcp_listen" => {
                        if parts.len() != 3 {
                            return Err(CapabilityError::InvalidFormat("network:tcp_listen requires port".to_string()));
                        }
                        let port = parts[2].parse::<u16>()
                            .map_err(|_| CapabilityError::InvalidFormat("invalid port number".to_string()))?;
                        NetworkCapability::TcpListen(port)
                    }
                    _ => return Err(CapabilityError::InvalidFormat(format!("unknown network operation: {}", parts[1]))),
                };
                Ok(CapabilityType::Network(net_cap))
            }
            _ => {
                // Custom capability
                if parts.len() != 3 {
                    return Err(CapabilityError::InvalidFormat("custom capability requires namespace:operation".to_string()));
                }
                Ok(CapabilityType::Custom(parts[1].to_string(), parts[2].to_string()))
            }
        }
    }
    
    /// Convert capability to string representation
    pub fn to_string(&self) -> String {
        match self {
            CapabilityType::ReadState(ns) => format!("read_state:{}", ns),
            CapabilityType::WriteState(ns) => format!("write_state:{}", ns),
            CapabilityType::DeleteState(ns) => format!("delete_state:{}", ns),
            CapabilityType::ListState(ns) => format!("list_state:{}", ns),
            CapabilityType::SendMessage(msg) => format!("send_msg:{}", msg),
            CapabilityType::ReceiveMessage(msg) => format!("receive_msg:{}", msg),
            CapabilityType::EmitEvent(evt) => format!("emit_event:{}", evt),
            CapabilityType::ExecuteModule(module) => format!("execute_module:{}", module),
            CapabilityType::AllocateMemory(size) => format!("allocate_memory:{}", size),
            CapabilityType::SystemInfo => "system_info".to_string(),
            CapabilityType::CreateCapability => "create_capability".to_string(),
            CapabilityType::DelegateCapability => "delegate_capability".to_string(),
            CapabilityType::Crypto(op) => match op {
                CryptoCapability::GenerateKeys => "crypto:generate_keys".to_string(),
                CryptoCapability::Sign => "crypto:sign".to_string(),
                CryptoCapability::Verify => "crypto:verify".to_string(),
                CryptoCapability::Hash => "crypto:hash".to_string(),
                CryptoCapability::Encrypt => "crypto:encrypt".to_string(),
            },
            CapabilityType::Network(op) => match op {
                NetworkCapability::HttpRequest(domain) => format!("network:http:{}", domain),
                NetworkCapability::TcpConnect(host, port) => format!("network:tcp_connect:{}:{}", host, port),
                NetworkCapability::TcpListen(port) => format!("network:tcp_listen:{}", port),
            },
            CapabilityType::Custom(ns, op) => format!("custom:{}:{}", ns, op),
        }
    }
    
    /// Check if this capability implies another capability
    /// Used for capability inheritance and hierarchical permissions
    pub fn implies(&self, other: &CapabilityType) -> bool {
        match (self, other) {
            // Write implies read for the same namespace
            (CapabilityType::WriteState(ns1), CapabilityType::ReadState(ns2)) => ns1 == ns2,
            
            // Delete implies write and read for the same namespace
            (CapabilityType::DeleteState(ns1), CapabilityType::WriteState(ns2)) => ns1 == ns2,
            (CapabilityType::DeleteState(ns1), CapabilityType::ReadState(ns2)) => ns1 == ns2,
            
            // Admin capabilities imply many things
            (CapabilityType::CreateCapability, _) => true, // Admin can do anything
            
            // Same capability implies itself
            (a, b) if a == b => true,
            
            _ => false,
        }
    }
}

/// A capability grant with metadata
#[derive(Debug, Clone)]
pub struct CapabilityGrant {
    /// The capability being granted
    pub capability: CapabilityType,
    
    /// Module that granted this capability
    pub granter: String,
    
    /// When the capability was granted
    pub granted_at: std::time::SystemTime,
    
    /// Optional expiration time
    pub expires_at: Option<std::time::SystemTime>,
    
    /// Whether this capability can be delegated
    pub delegatable: bool,
}

impl PartialEq for CapabilityGrant {
    fn eq(&self, other: &Self) -> bool {
        self.capability == other.capability && self.granter == other.granter
    }
}

impl Eq for CapabilityGrant {}

impl std::hash::Hash for CapabilityGrant {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.capability.hash(state);
        self.granter.hash(state);
    }
}

/// Module capability manager
pub struct CapabilityManager {
    /// Capabilities granted to each module
    module_capabilities: Arc<Mutex<HashMap<String, HashSet<CapabilityGrant>>>>,
    
    /// Capability delegation chains
    delegation_chains: Arc<Mutex<HashMap<String, Vec<String>>>>,
    
    /// System capabilities (granted by the system, not modules)
    system_capabilities: Arc<Mutex<HashSet<CapabilityType>>>,
}

impl CapabilityManager {
    /// Create a new capability manager
    pub fn new() -> Self {
        let mut system_caps = HashSet::new();
        
        // Define system-level capabilities that can be granted
        system_caps.insert(CapabilityType::SystemInfo);
        system_caps.insert(CapabilityType::CreateCapability);
        system_caps.insert(CapabilityType::DelegateCapability);
        
        Self {
            module_capabilities: Arc::new(Mutex::new(HashMap::new())),
            delegation_chains: Arc::new(Mutex::new(HashMap::new())),
            system_capabilities: Arc::new(Mutex::new(system_caps)),
        }
    }
    
    /// Grant a capability to a module
    pub fn grant_capability(
        &self,
        module: &str,
        capability: CapabilityType,
        granter: &str,
        delegatable: bool,
    ) -> Result<()> {
        debug!("Granting capability {:?} to module {} by {}", capability, module, granter);
        
        // Check if granter has the capability to grant
        if granter != "system" && !self.has_capability(granter, &CapabilityType::CreateCapability)? {
            // Check if granter has the capability they're trying to grant
            if !self.has_capability(granter, &capability)? {
                return Err(CapabilityError::NotGranted(
                    format!("{} cannot grant capability it doesn't have", granter)
                ));
            }
            
            // Check if the capability is delegatable
            if !self.is_capability_delegatable(granter, &capability)? {
                return Err(CapabilityError::InvalidDelegation(
                    format!("Capability is not delegatable by {}", granter)
                ));
            }
        }
        
        let grant = CapabilityGrant {
            capability: capability.clone(),
            granter: granter.to_string(),
            granted_at: std::time::SystemTime::now(),
            expires_at: None,
            delegatable,
        };
        
        let mut caps = self.module_capabilities.lock()
            .map_err(|e| CapabilityError::LockPoisoned(e.to_string()))?;
        
        caps.entry(module.to_string())
            .or_insert_with(HashSet::new)
            .insert(grant);
        
        info!("Granted capability {} to module {}", capability.to_string(), module);
        Ok(())
    }
    
    /// Revoke a capability from a module
    pub fn revoke_capability(&self, module: &str, capability: &CapabilityType) -> Result<()> {
        debug!("Revoking capability {:?} from module {}", capability, module);
        
        let mut caps = self.module_capabilities.lock()
            .map_err(|e| CapabilityError::LockPoisoned(e.to_string()))?;
        
        if let Some(module_caps) = caps.get_mut(module) {
            module_caps.retain(|grant| &grant.capability != capability);
            
            if module_caps.is_empty() {
                caps.remove(module);
            }
        }
        
        info!("Revoked capability {} from module {}", capability.to_string(), module);
        Ok(())
    }
    
    /// Check if a module has a specific capability
    pub fn has_capability(&self, module: &str, capability: &CapabilityType) -> Result<bool> {
        let caps = self.module_capabilities.lock()
            .map_err(|e| CapabilityError::LockPoisoned(e.to_string()))?;
        
        if let Some(module_caps) = caps.get(module) {
            // Check direct capabilities
            for grant in module_caps {
                // Check expiration
                if let Some(expires) = grant.expires_at {
                    if expires < std::time::SystemTime::now() {
                        continue;
                    }
                }
                
                // Check if granted capability matches or implies requested capability
                if &grant.capability == capability || grant.capability.implies(capability) {
                    return Ok(true);
                }
            }
        }
        
        Ok(false)
    }
    
    /// Check if a capability is delegatable by a module
    fn is_capability_delegatable(&self, module: &str, capability: &CapabilityType) -> Result<bool> {
        let caps = self.module_capabilities.lock()
            .map_err(|e| CapabilityError::LockPoisoned(e.to_string()))?;
        
        if let Some(module_caps) = caps.get(module) {
            for grant in module_caps {
                if &grant.capability == capability {
                    return Ok(grant.delegatable);
                }
            }
        }
        
        Ok(false)
    }
    
    /// Require a capability, returning an error if not granted
    pub fn require_capability(&self, module: &str, capability: &CapabilityType) -> Result<()> {
        if !self.has_capability(module, capability)? {
            return Err(CapabilityError::NotGranted(
                format!("Module {} lacks capability: {}", module, capability.to_string())
            ));
        }
        Ok(())
    }
    
    /// List all capabilities for a module
    pub fn list_capabilities(&self, module: &str) -> Result<Vec<CapabilityType>> {
        let caps = self.module_capabilities.lock()
            .map_err(|e| CapabilityError::LockPoisoned(e.to_string()))?;
        
        if let Some(module_caps) = caps.get(module) {
            Ok(module_caps.iter()
                .filter(|grant| {
                    // Filter out expired capabilities
                    if let Some(expires) = grant.expires_at {
                        expires >= std::time::SystemTime::now()
                    } else {
                        true
                    }
                })
                .map(|grant| grant.capability.clone())
                .collect())
        } else {
            Ok(Vec::new())
        }
    }
    
    /// Grant a set of default capabilities to a new module
    pub fn grant_default_capabilities(&self, module: &str) -> Result<()> {
        debug!("Granting default capabilities to module {}", module);
        
        // Default capabilities for all modules
        self.grant_capability(module, CapabilityType::SystemInfo, "system", false)?;
        self.grant_capability(
            module,
            CapabilityType::AllocateMemory(16 * 1024 * 1024), // 16MB default
            "system",
            false,
        )?;
        
        Ok(())
    }
    
    /// Create a capability delegation chain
    pub fn delegate_capability(
        &self,
        from_module: &str,
        to_module: &str,
        capability: &CapabilityType,
    ) -> Result<()> {
        debug!("Delegating capability {:?} from {} to {}", capability, from_module, to_module);
        
        // Check delegation permission
        self.require_capability(from_module, &CapabilityType::DelegateCapability)?;
        
        // Check if from_module has the capability
        if !self.has_capability(from_module, capability)? {
            return Err(CapabilityError::NotGranted(
                format!("{} cannot delegate capability it doesn't have", from_module)
            ));
        }
        
        // Check if capability is delegatable
        if !self.is_capability_delegatable(from_module, capability)? {
            return Err(CapabilityError::InvalidDelegation(
                "Capability is not delegatable".to_string()
            ));
        }
        
        // Check for circular dependencies
        let mut chains = self.delegation_chains.lock()
            .map_err(|e| CapabilityError::LockPoisoned(e.to_string()))?;
        
        // Simple cycle detection
        let mut visited = HashSet::new();
        let mut current = to_module;
        visited.insert(from_module);
        
        while let Some(chain) = chains.get(current) {
            if chain.contains(&from_module.to_string()) {
                return Err(CapabilityError::CircularDependency);
            }
            for module in chain {
                if !visited.insert(module) {
                    return Err(CapabilityError::CircularDependency);
                }
            }
            current = chain.last().map(|s| s.as_str()).unwrap_or("");
        }
        
        // Record delegation
        chains.entry(to_module.to_string())
            .or_insert_with(Vec::new)
            .push(from_module.to_string());
        
        // Grant the capability with delegation flag
        self.grant_capability(to_module, capability.clone(), from_module, true)?;
        
        info!("Delegated capability {} from {} to {}", capability.to_string(), from_module, to_module);
        Ok(())
    }
    
    /// Check module access to a resource
    pub fn check_access(&self, module: &str, resource: &str, operation: &str) -> Result<()> {
        debug!("Checking access for module {} to resource {} operation {}", module, resource, operation);
        
        // Map resource and operation to required capability
        let required_cap = match operation {
            "read" => CapabilityType::ReadState(resource.to_string()),
            "write" => CapabilityType::WriteState(resource.to_string()),
            "delete" => CapabilityType::DeleteState(resource.to_string()),
            "list" => CapabilityType::ListState(resource.to_string()),
            _ => return Err(CapabilityError::InvalidFormat(
                format!("Unknown operation: {}", operation)
            )),
        };
        
        self.require_capability(module, &required_cap)
    }
}

impl Default for CapabilityManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_capability_parsing() {
        // Test valid capabilities
        let cap = CapabilityType::from_string("read_state:auth").unwrap();
        assert!(matches!(cap, CapabilityType::ReadState(ns) if ns == "auth"));
        
        let cap = CapabilityType::from_string("write_state:bank").unwrap();
        assert!(matches!(cap, CapabilityType::WriteState(ns) if ns == "bank"));
        
        let cap = CapabilityType::from_string("send_msg:/cosmos.bank.v1beta1.MsgSend").unwrap();
        assert!(matches!(cap, CapabilityType::SendMessage(msg) if msg == "/cosmos.bank.v1beta1.MsgSend"));
        
        let cap = CapabilityType::from_string("allocate_memory:1048576").unwrap();
        assert!(matches!(cap, CapabilityType::AllocateMemory(size) if size == 1048576));
        
        let cap = CapabilityType::from_string("crypto:sign").unwrap();
        assert!(matches!(cap, CapabilityType::Crypto(CryptoCapability::Sign)));
        
        let cap = CapabilityType::from_string("network:http:api.example.com").unwrap();
        assert!(matches!(cap, CapabilityType::Network(NetworkCapability::HttpRequest(domain)) if domain == "api.example.com"));
        
        // Test invalid capabilities
        assert!(CapabilityType::from_string("").is_err());
        assert!(CapabilityType::from_string("read_state").is_err());
        assert!(CapabilityType::from_string("invalid:too:many:parts:here").is_err());
    }
    
    #[test]
    fn test_capability_implications() {
        let write_cap = CapabilityType::WriteState("auth".to_string());
        let read_cap = CapabilityType::ReadState("auth".to_string());
        let delete_cap = CapabilityType::DeleteState("auth".to_string());
        
        // Write implies read for same namespace
        assert!(write_cap.implies(&read_cap));
        
        // Delete implies both write and read
        assert!(delete_cap.implies(&write_cap));
        assert!(delete_cap.implies(&read_cap));
        
        // But not for different namespaces
        let other_read = CapabilityType::ReadState("bank".to_string());
        assert!(!write_cap.implies(&other_read));
        
        // Admin capability implies everything
        let admin_cap = CapabilityType::CreateCapability;
        assert!(admin_cap.implies(&read_cap));
        assert!(admin_cap.implies(&write_cap));
        assert!(admin_cap.implies(&delete_cap));
    }
    
    #[test]
    fn test_capability_manager() {
        let manager = CapabilityManager::new();
        
        // Grant some capabilities
        manager.grant_capability("module_a", CapabilityType::ReadState("auth".to_string()), "system", true).unwrap();
        manager.grant_capability("module_a", CapabilityType::WriteState("auth".to_string()), "system", false).unwrap();
        
        // Check capabilities
        assert!(manager.has_capability("module_a", &CapabilityType::ReadState("auth".to_string())).unwrap());
        assert!(manager.has_capability("module_a", &CapabilityType::WriteState("auth".to_string())).unwrap());
        assert!(!manager.has_capability("module_a", &CapabilityType::ReadState("bank".to_string())).unwrap());
        
        // Test implied capabilities
        // Write implies read, so module_a should have read even though we check via write
        assert!(manager.has_capability("module_a", &CapabilityType::ReadState("auth".to_string())).unwrap());
        
        // Test revocation
        manager.revoke_capability("module_a", &CapabilityType::WriteState("auth".to_string())).unwrap();
        assert!(!manager.has_capability("module_a", &CapabilityType::WriteState("auth".to_string())).unwrap());
        // But read should still be there (was granted separately)
        assert!(manager.has_capability("module_a", &CapabilityType::ReadState("auth".to_string())).unwrap());
    }
    
    #[test]
    fn test_capability_delegation() {
        let manager = CapabilityManager::new();
        
        // Grant capabilities to module_a
        manager.grant_capability("module_a", CapabilityType::ReadState("auth".to_string()), "system", true).unwrap();
        manager.grant_capability("module_a", CapabilityType::DelegateCapability, "system", false).unwrap();
        
        // Module A delegates read capability to Module B
        manager.delegate_capability("module_a", "module_b", &CapabilityType::ReadState("auth".to_string())).unwrap();
        
        // Module B should now have the capability
        assert!(manager.has_capability("module_b", &CapabilityType::ReadState("auth".to_string())).unwrap());
        
        // Test that non-delegatable capabilities cannot be delegated
        manager.grant_capability("module_c", CapabilityType::WriteState("bank".to_string()), "system", false).unwrap();
        manager.grant_capability("module_c", CapabilityType::DelegateCapability, "system", false).unwrap();
        
        let result = manager.delegate_capability("module_c", "module_d", &CapabilityType::WriteState("bank".to_string()));
        assert!(result.is_err());
    }
    
    #[test]
    fn test_access_control() {
        let manager = CapabilityManager::new();
        
        // Grant capabilities
        manager.grant_capability("bank_module", CapabilityType::ReadState("bank".to_string()), "system", false).unwrap();
        manager.grant_capability("bank_module", CapabilityType::WriteState("bank".to_string()), "system", false).unwrap();
        
        // Check access
        assert!(manager.check_access("bank_module", "bank", "read").is_ok());
        assert!(manager.check_access("bank_module", "bank", "write").is_ok());
        assert!(manager.check_access("bank_module", "auth", "read").is_err());
        assert!(manager.check_access("bank_module", "bank", "delete").is_err()); // No delete capability
    }
    
    #[test]
    fn test_default_capabilities() {
        let manager = CapabilityManager::new();
        
        // Grant default capabilities
        manager.grant_default_capabilities("new_module").unwrap();
        
        // Check that default capabilities are granted
        assert!(manager.has_capability("new_module", &CapabilityType::SystemInfo).unwrap());
        assert!(manager.has_capability("new_module", &CapabilityType::AllocateMemory(16 * 1024 * 1024)).unwrap());
        
        // List capabilities
        let caps = manager.list_capabilities("new_module").unwrap();
        assert_eq!(caps.len(), 2);
    }
}