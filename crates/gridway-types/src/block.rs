//! Gridway block type implementing Commonware consensus traits.

use bytes::{Buf, BufMut};
use commonware_codec::{varint::UInt, EncodeSize, Error, Read, ReadExt, Write};
use commonware_consensus::{types::Height, Heightable};
use commonware_cryptography::{sha256::Digest, Committable, Digestible, Hasher, Sha256};

/// A Gridway block containing transactions and state root.
///
/// Implements all required Commonware traits:
/// - `commonware_consensus::Block` (parent reference)
/// - `Heightable` (block height)
/// - `Digestible` (content hash)
/// - `Committable` (commitment for consensus)
/// - `Write`/`Read` (codec serialization)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridwayBlock {
    /// Parent block digest
    pub parent: Digest,
    /// Block height
    pub height: Height,
    /// Block timestamp (milliseconds since Unix epoch)
    pub timestamp: u64,
    /// State root hash after executing this block's transactions
    pub state_root: [u8; 32],
    /// Serialized transactions in this block
    pub transactions: Vec<Vec<u8>>,
    /// Pre-computed digest
    digest: Digest,
}

impl GridwayBlock {
    fn compute_digest(
        parent: &Digest,
        height: Height,
        timestamp: u64,
        state_root: &[u8; 32],
        transactions: &[Vec<u8>],
    ) -> Digest {
        let mut hasher = Sha256::new();
        hasher.update(parent);
        hasher.update(&height.get().to_be_bytes());
        hasher.update(&timestamp.to_be_bytes());
        hasher.update(state_root);
        // Hash number of transactions
        hasher.update(&(transactions.len() as u32).to_be_bytes());
        // Hash each transaction
        for tx in transactions {
            hasher.update(&(tx.len() as u32).to_be_bytes());
            hasher.update(tx);
        }
        hasher.finalize()
    }

    /// Create a new block
    pub fn new(
        parent: Digest,
        height: Height,
        timestamp: u64,
        state_root: [u8; 32],
        transactions: Vec<Vec<u8>>,
    ) -> Self {
        let digest = Self::compute_digest(&parent, height, timestamp, &state_root, &transactions);
        Self {
            parent,
            height,
            timestamp,
            state_root,
            transactions,
            digest,
        }
    }

    /// Create a genesis block
    pub fn genesis() -> Self {
        let genesis_hash = {
            let mut h = Sha256::new();
            h.update(b"gridway genesis");
            h.finalize()
        };
        Self::new(genesis_hash, Height::zero(), 0, [0u8; 32], vec![])
    }
}

impl Write for GridwayBlock {
    fn write(&self, writer: &mut impl BufMut) {
        self.parent.write(writer);
        self.height.write(writer);
        UInt(self.timestamp).write(writer);
        writer.put_slice(&self.state_root);
        // Write transactions
        UInt(self.transactions.len() as u64).write(writer);
        for tx in &self.transactions {
            UInt(tx.len() as u64).write(writer);
            writer.put_slice(tx);
        }
    }
}

impl Read for GridwayBlock {
    type Cfg = ();

    fn read_cfg(reader: &mut impl Buf, _: &Self::Cfg) -> Result<Self, Error> {
        let parent = Digest::read(reader)?;
        let height = Height::read(reader)?;
        let timestamp: u64 = UInt::read(reader)?.into();

        // Read state root
        if reader.remaining() < 32 {
            return Err(Error::EndOfBuffer);
        }
        let mut state_root = [0u8; 32];
        reader.copy_to_slice(&mut state_root);

        // Read transactions
        let tx_count: u64 = UInt::read(reader)?.into();
        let mut transactions = Vec::with_capacity(tx_count as usize);
        for _ in 0..tx_count {
            let tx_len: u64 = UInt::read(reader)?.into();
            if reader.remaining() < tx_len as usize {
                return Err(Error::EndOfBuffer);
            }
            let mut tx = vec![0u8; tx_len as usize];
            reader.copy_to_slice(&mut tx);
            transactions.push(tx);
        }

        let digest = Self::compute_digest(&parent, height, timestamp, &state_root, &transactions);
        Ok(Self {
            parent,
            height,
            timestamp,
            state_root,
            transactions,
            digest,
        })
    }
}

impl EncodeSize for GridwayBlock {
    fn encode_size(&self) -> usize {
        self.parent.encode_size()
            + self.height.encode_size()
            + UInt(self.timestamp).encode_size()
            + 32 // state_root
            + UInt(self.transactions.len() as u64).encode_size()
            + self.transactions.iter().map(|tx| {
                UInt(tx.len() as u64).encode_size() + tx.len()
            }).sum::<usize>()
    }
}

impl Digestible for GridwayBlock {
    type Digest = Digest;

    fn digest(&self) -> Digest {
        self.digest
    }
}

impl Committable for GridwayBlock {
    type Commitment = Digest;

    fn commitment(&self) -> Digest {
        self.digest
    }
}

impl commonware_consensus::Block for GridwayBlock {
    fn parent(&self) -> Digest {
        self.parent
    }
}

impl Heightable for GridwayBlock {
    fn height(&self) -> Height {
        self.height
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonware_codec::{DecodeExt, Encode};

    #[test]
    fn test_genesis_block() {
        let genesis = GridwayBlock::genesis();
        assert_eq!(genesis.height(), Height::zero());
        assert_eq!(genesis.transactions.len(), 0);
    }

    #[test]
    fn test_block_roundtrip() {
        let block = GridwayBlock::new(
            Digest([0u8; 32]),
            Height::new(1),
            1234567890,
            [42u8; 32],
            vec![vec![1, 2, 3], vec![4, 5, 6]],
        );

        let encoded = block.encode();
        let decoded = GridwayBlock::decode(encoded).unwrap();
        assert_eq!(block, decoded);
    }

    #[test]
    fn test_block_digest_determinism() {
        let block1 = GridwayBlock::new(
            Digest([0u8; 32]),
            Height::new(1),
            1000,
            [0u8; 32],
            vec![vec![1, 2, 3]],
        );
        let block2 = GridwayBlock::new(
            Digest([0u8; 32]),
            Height::new(1),
            1000,
            [0u8; 32],
            vec![vec![1, 2, 3]],
        );
        assert_eq!(block1.digest(), block2.digest());
    }

    #[test]
    fn test_block_different_txs_different_digest() {
        let block1 = GridwayBlock::new(
            Digest([0u8; 32]),
            Height::new(1),
            1000,
            [0u8; 32],
            vec![vec![1, 2, 3]],
        );
        let block2 = GridwayBlock::new(
            Digest([0u8; 32]),
            Height::new(1),
            1000,
            [0u8; 32],
            vec![vec![4, 5, 6]],
        );
        assert_ne!(block1.digest(), block2.digest());
    }
}
