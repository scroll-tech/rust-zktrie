use std::fmt::Debug;
use crate::raw::ImplError;
use num_derive::FromPrimitive;
use num;

const HASH_BYTE_LEN: usize = 32;

pub trait Hashable : Clone + Debug + Default + PartialEq {
    fn hash_elems_with_domain(domain: u64, lbytes: &Option<Self>, rbytes: &Option<Self>) -> Result<Self, ImplError>;
    fn handling_elems_and_bytes32(flags: u32, bytes: &Vec<[u8; 32]>) -> Result<Self, ImplError>;
    fn hash_from_bytes(bytes: &Vec<u8>) -> Result<Self, ImplError>;
    fn hash_zero() -> Self;
    fn check_in_field(hash: &Self) -> bool;
    fn test_bit(key: &Self, pos: usize) -> bool;
    fn to_bytes(&self) -> Vec<u8>;
}


#[derive (Copy, Clone, Debug, FromPrimitive, Display, PartialEq)]
pub enum NodeType {
	// NodeTypeParent indicates the type of parent Node that has children.
	NodeTypeParent = 0,
	// NodeTypeLeaf indicates the type of a leaf Node that contains a key &
	// value.
	NodeTypeLeaf = 1,
	// NodeTypeEmpty indicates the type of an empty Node.
	NodeTypeEmpty = 2,

	// DBEntryTypeRoot indicates the type of a DB entry that indicates the
	// current Root of a MerkleTree
	DBEntryTypeRoot = 3,

	NodeTypeLeafNew = 4,
	NodeTypeEmptyNew = 5,
	// branch node for both child are terminal nodes
	NodeTypeBranch0 = 6,
	// branch node for left child is terminal node and right child is branch
	NodeTypeBranch1 = 7,
	// branch node for left child is branch node and right child is terminal
	NodeTypeBranch2 = 8,
	// branch node for both child are branch nodes
	NodeTypeBranch3 = 9,
    // any invalid situation
    NodeTypeInvalid = 10,
}

use NodeType::*;
use strum_macros::Display;

impl NodeType {
    /// deduce a new branch type from current branch when one of its child become non trivial
    pub fn deduce_upgrade_type(&self, is_right: bool) -> Self {
        if is_right {
            match self {
                NodeTypeBranch0 => NodeTypeBranch1,
                NodeTypeBranch1 => self.clone(),
                NodeTypeBranch2 => NodeTypeBranch3,
                NodeTypeBranch3 => NodeTypeBranch3,
                _ => unreachable!(),
            }
        } else {
            match self {
                NodeTypeBranch0 => NodeTypeBranch2,
                NodeTypeBranch1 => NodeTypeBranch3,
                NodeTypeBranch3 => NodeTypeBranch3,
                NodeTypeBranch2 => self.clone(),
                _ => unreachable!(),
		    }
        }
    }


    /// deduce a new branch type from current branch when one of its child become terminal
    pub fn deduce_downgrade_type(&self, is_right: bool) -> Self {
        if is_right {
            match self {
                NodeTypeBranch1 => NodeTypeBranch0,
                NodeTypeBranch3 => NodeTypeBranch2,
                _ => {
                    panic!("can not downgrade a node with terminal child {}", self);
                }
            }
        } else {
            match self {
                NodeTypeBranch3 => NodeTypeBranch1,
                NodeTypeBranch2 => NodeTypeBranch0,
                _ => {
                    panic!("can not downgrade a node with terminal child {}", self);
                }
		    }
        }
    }
}

// Node is the struct that represents a node in the MT. The node should not be
// modified after creation because the cached key won't be updated.
#[derive (Clone)]
pub struct Node<H: Hashable> {
	// node_type is the type of node in the tree.
	pub node_type: NodeType,
	// child_l is the node hash of the left child of a parent node.
    pub child_left: Option<H>,
	// child_r is the node hash of the right child of a parent node.
	pub child_right: Option<H>,
	// key is the node's key stored in a leaf node.
	pub node_key: H,
	// value_preimage can store at most 256 byte32 as fields (represnted by BIG-ENDIAN integer)
	// and the first 24 can be compressed (each bytes32 consider as 2 fields), in hashing the compressed
	// elemments would be calculated first
	pub value_preimage: Vec<[u8; 32]>,
	// use each bit for indicating the compressed flag for the first 24 fields
	compress_flags: u32,
	// nodeHash is the cache of the hash of the node to avoid recalculating
	pub node_hash: Option<H>,
	// valueHash is the cache of the hash of valuePreimage to avoid recalculating, only valid for leaf node
	value_hash: Option<H>,
	// KeyPreimage is the original key value that derives the node_key, kept here only for proof
	key_preimage: Option<[u8; 32]>,
}

impl<H: Hashable> Node<H> {
    /// create a new leaf node
    pub fn new_leaf_node(node_key: H, value_flags: u32, value_preimage: Vec<[u8; 32]>) -> Self {
        Node {
	       node_type: NodeType::NodeTypeLeafNew,
           node_key,
           compress_flags: value_flags,
           value_preimage,
           child_left: None,
           child_right: None,
           node_hash: None,
           value_hash: None,
           key_preimage: None,
        }
    }

    /// creates a new parent node.
    pub fn new_parent_node(node_type: NodeType, child_left: H, child_right: H) -> Self {
        Node {
	       node_type,
           node_key: H::default(),
           compress_flags: 0,
           value_preimage: vec![],
           child_left: Some(child_left),
           child_right: Some(child_right),
           node_hash: None,
           value_hash: None,
           key_preimage: None,
        }
    }

    /// creates a new empty node.
    pub fn new_empty_node() -> Self {
        Node {
	       node_type: NodeType::NodeTypeEmptyNew,
           node_key: H::default(),
           compress_flags: 0,
           value_preimage: vec![],
           child_left: None,
           child_right: None,
           node_hash: None,
           value_hash: None,
           key_preimage: None,
        }
    }

    // new_node_from_bytes creates a new node by parsing the input []byte.
    pub fn new_node_from_bytes(b: Vec<u8>) -> Result<Node<H>, ImplError> {
    	if b.len() < 1 {
    		Err(ImplError::ErrNodeBytesBadSize)
    	} else {
            let mut node =  Node::new_empty_node();
    	    node.node_type = num::FromPrimitive::from_u32(b[0] as u32).unwrap_or(NodeTypeInvalid);
    	    let b = b[1..].to_vec();
            match node.node_type {
                NodeTypeParent | NodeTypeBranch0 |
                NodeTypeBranch1 |NodeTypeBranch2 | NodeTypeBranch3 => {
                    if b.len() != 2 * HASH_BYTE_LEN {
                    	Err(ImplError::ErrNodeBytesBadSize)
                    } else {
                        node.child_left = Some(H::hash_from_bytes(&b[..HASH_BYTE_LEN].to_vec())?);
                        node.child_right = Some(H::hash_from_bytes(&b[HASH_BYTE_LEN..HASH_BYTE_LEN*2].to_vec())?);
                        Ok(node)
                    }
                },
                NodeTypeLeaf | NodeTypeLeafNew => {
                    if b.len() < HASH_BYTE_LEN + 4 {
                        Err(ImplError::ErrNodeBytesBadSize)
                    } else {
                        node.node_key = H::hash_from_bytes(&b[..HASH_BYTE_LEN].to_vec())?;
                        let mark = u32::from_le_bytes(b[HASH_BYTE_LEN..HASH_BYTE_LEN+4].try_into().unwrap());
                        let preimage_len = (mark & 255) as usize;
                        node.compress_flags = mark >> 8;
                        let mut cur_pos = HASH_BYTE_LEN + 4;
                        if b.len() < cur_pos + preimage_len * 32 + 1 {
                            Err(ImplError::ErrNodeBytesBadSize)
                        } else {
                            for i in 0..preimage_len as usize {
                                let a = &b[i*32 + cur_pos..(i+1)*32 + cur_pos];
                                node.value_preimage.push(a.try_into().unwrap());
                            }
                            cur_pos += preimage_len * 32;
                            let preimage_size = b[cur_pos] as usize;
                            cur_pos += 1;
                            if preimage_size != 0 {
                                if b.len() < cur_pos + preimage_size || preimage_size != 32 {
                                    Err(ImplError::ErrNodeBytesBadSize)
                                } else {
                                    let a = &b[cur_pos..cur_pos + preimage_size];
                                    node.key_preimage = Some(a.try_into().unwrap());
                                    Ok(node)
                                }
                            } else {
                                Ok(node)
                            }
                        }

                    }
                },
                NodeTypeEmpty | NodeTypeEmptyNew => {
                    Ok(node)
                },
                _ => Err(ImplError::ErrInvalidNodeFound)

            }
        }
    }
    /// is_terminal returns if the node is 'terminated', i.e. empty or leaf node
    pub fn is_terminal(&self) -> bool {
    	match self.node_type {
    	    NodeTypeEmptyNew | NodeTypeLeafNew => true,
    	    NodeTypeBranch0 | NodeTypeBranch1 | NodeTypeBranch2 | NodeTypeBranch3 => false,
    	    NodeTypeEmpty | NodeTypeLeaf | NodeTypeParent => panic!("encounter deprecated node types"),
            _ => panic!("encounter unknown node types {:?}", self.node_type)
    	}
    }

    /// NodeHash computes the hash digest of the node by hashing the content in a
    /// specific way for each type of node.  This key is used as the hash of the
    /// Merkle tree for each node.
    pub fn node_hash(&mut self) -> Result<H, ImplError> {
    	if self.node_hash.is_none() { // Cache the key to avoid repeated hash computations.
        // NOTE: We are not using the type to calculate the hash!
            match self.node_type {
                NodeTypeBranch0
                | NodeTypeBranch1
                | NodeTypeBranch2
                | NodeTypeBranch3  => {// H(ChildL || ChildR)
                    self.node_hash = Some(H::hash_elems_with_domain (
                        self.node_type as u64,
                        &self.child_left,
                        &self.child_right,
                    )?);
                },
                NodeTypeLeafNew => {
                	self.value_hash = Some(H::handling_elems_and_bytes32(
                        self.compress_flags,
                        &self.value_preimage
                    )?);
                    self.node_hash = Some(self.leaf_hash(&Some(self.node_key.clone()), &self.value_hash)?);
                },
                NodeTypeEmptyNew => { // Zero
                    self.node_hash = Some(H::hash_zero());
                }
                NodeTypeEmpty | NodeTypeLeaf | NodeTypeParent =>
                	panic!("encounter deprecated node types"),
                _ => self.node_hash = Some(H::hash_zero())
            }
    	}
        Ok(self.node_hash.as_ref().unwrap().clone())
    }

    /// ValueHash computes the hash digest of the value stored in the leaf node. For
    /// other node types, it returns the zero hash.
    pub fn value_hash(&mut self) -> Result<H, ImplError> {
        match self.node_type {
            NodeTypeLeafNew => {
                self.node_hash()?;
                Ok(self.value_hash.as_ref().unwrap().clone())
            },
            _ => {
                Ok(H::hash_zero())
            }
        }
    }

    /// LeafHash computes the key of a leaf node given the hIndex and hValue of the
    /// entry of the leaf.
    pub fn leaf_hash(&self, k: &Option<H>, v: &Option<H>) -> Result<H, ImplError> {
        H::hash_elems_with_domain(self.node_type as u64, k, v)
    }

    /// Data returns the wrapped data inside LeafNode and cast them into bytes
    /// for other node type it just return None
    pub fn data(&self) -> Option<Vec<u8>> {
        match self.node_type {
            NodeTypeLeafNew => unsafe {
                let slice = std::slice::from_raw_parts(self.value_preimage.as_ptr() as *const [u8;32], self.value_preimage.len());
                Some(slice.flatten().to_vec())
            },
            _ => None
        }
	}

    // Value returns the encoded bytes of a node, include all information of it
    pub fn value(&self) -> Vec<u8> {
        let mut out_bytes = self.canonical_value();
        let len = out_bytes.len();
        match self.node_type {
            NodeTypeLeafNew => {
                if self.key_preimage.is_some() {
                    let key_preimage = self.key_preimage.unwrap();
			        out_bytes[len-1] = key_preimage.len() as u8;
                    let bytes = unsafe { std::slice::from_raw_parts(key_preimage.as_ptr() as *const u8, key_preimage.len()) };
                    out_bytes.append(&mut bytes.to_vec())
                }
            },
            _ => {}
		}
	    out_bytes
	}

    /// CanonicalValue returns the byte form of a node required to be persisted, and strip unnecessary fields
    /// from the encoding (current only KeyPreimage for Leaf node) to keep a minimum size for content being
    /// stored in backend storage
    pub fn canonical_value(&self) -> Vec<u8> {
        match self.node_type {
            NodeTypeBranch0 | NodeTypeBranch1 | NodeTypeBranch2 | NodeTypeBranch3 => {
                let mut b =vec![self.node_type as u8];
                b.append(&mut self.child_left.as_ref().unwrap().to_bytes());
                b.append(&mut self.child_right.as_ref().unwrap().to_bytes());
                b
            },
            NodeTypeLeafNew => {
                let mut b = vec![self.node_type as u8];
                b.append(&mut self.node_key.to_bytes());
                let mark = (self.compress_flags << 8) + self.value_preimage.len() as u32;
                b.append(&mut u32::to_le_bytes(mark).to_vec());
                for i in 0..self.value_preimage.len() {
                    b.append(&mut self.value_preimage[i].to_vec());
                }
                b.push(0);
                b
            },
            NodeTypeEmptyNew => {
                vec![self.node_type as u8]
            },
            NodeTypeEmpty | NodeTypeLeaf | NodeTypeParent => {
                panic!("encounter deprecated node types")
            },
            _ => {vec![]}
        }
    }

    /// String outputs a string representation of a node (different for each type).
    pub fn to_string(&self) -> String {
        match self.node_type {
        // {Type || ChildL || ChildR}
            NodeTypeBranch0 =>
            	format!("Parent L(t):{:?} R(t):{:?}", self.child_left, self.child_right),
            NodeTypeBranch1 =>
            	format!("Parent L(t):{:?} R:{:?}", self.child_left, self.child_right),
            NodeTypeBranch2 =>
            	format!("Parent L:{:?} R(t):{:?}", self.child_left, self.child_right),
            NodeTypeBranch3 =>
            	format!("Parent L:{:?} R:{:?}", self.child_left, self.child_right),
            NodeTypeLeafNew => // {Type || Data...}
            	format!("Leaf I:{:?} Items: {}, First:{:?}",
                        self.node_key,
                        self.value_preimage.len(),
                        self.value_preimage[0]),
            NodeTypeEmptyNew => // {}
            	"Empty".to_string(),
            NodeTypeEmpty | NodeTypeLeaf | NodeTypeParent =>
            	"deprecated Node".to_string(),
            _ => "Invalid Node".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::hash::Hash;
    use crate::raw::ImplError;
    use crate::types::{NodeType::*, HASH_BYTE_LEN};
    use crate::types::{Node, Hashable};

    #[test]
    fn test_new_node() {
        //NodeTypeEmptyNew
        let mut node1 = Node::<Hash>::new_empty_node();
        assert_eq!(node1.node_type, NodeTypeEmptyNew);

        let h = node1.node_hash();
        assert!(h.is_ok());
        assert_eq!(h.unwrap(), Hash::hash_zero());
        let h = node1.value_hash();
        assert!(h.is_ok());
        assert_eq!(h.unwrap(), Hash::hash_zero());

        //NodeTypeLeafNew
        let k = Hash::hash_from_bytes(&[47u8; 32].to_vec()).unwrap();
        let vp = vec![[48u8; 32]];
        let mut node2 = Node::<Hash>::new_leaf_node(k, 1, vp.clone());
        assert_eq!(node2.node_type, NodeTypeLeafNew);
        assert_eq!(node2.compress_flags, 1u32);
        assert_eq!(node2.value_preimage, vp);

        let h = node2.node_hash();
        assert!(h.is_ok());
        let h = node2.value_hash();
        assert!(h.is_ok());

        //New Parent Node
        let k = Hash::hash_from_bytes(&[47u8; 32].to_vec()).unwrap();
        let mut node3 = Node::<Hash>::new_parent_node(NodeTypeBranch3, k.clone(), k.clone());
        assert_eq!(node3.node_type, NodeTypeBranch3);
        assert_eq!(node3.child_left.as_ref().unwrap(), &k);
        assert_eq!(node3.child_right.as_ref().unwrap(), &k);

        let h = node3.node_hash();
        assert!(h.is_ok());
        let h = node3.value_hash();
        assert!(h.is_ok());

        //New Parent Node with empty child
        let k = Hash::hash_from_bytes(&[47u8; 32].to_vec()).unwrap();
        let r = Hash::hash_zero();
        let mut node4 = Node::<Hash>::new_parent_node(NodeTypeBranch2, k.clone(), r.clone());
        assert_eq!(node4.node_type, NodeTypeBranch2);
        assert_eq!(node4.child_left.as_ref().unwrap(), &k);
        assert_eq!(node4.child_right.as_ref().unwrap(), &r);

        let h = node4.node_hash();
        assert!(h.is_ok());
        let h = node4.value_hash();
        assert!(h.is_ok());

    }

    #[test]
    fn test_new_node_from_bytes() {
        //Parent Node
        let k1 = Hash::hash_from_bytes(&[47u8; 32].to_vec()).unwrap();
        let k2 = Hash::hash_from_bytes(&[48u8; 32].to_vec()).unwrap();
        let mut node1 = Node::<Hash>::new_parent_node(NodeTypeBranch0, k1.clone(), k2.clone());
        assert_eq!(node1.node_type, NodeTypeBranch0);
        assert_eq!(node1.child_left.as_ref().unwrap(), &k1);
        assert_eq!(node1.child_right.as_ref().unwrap(), &k2);

        let h = node1.node_hash();
        assert!(h.is_ok());
        let h = node1.value_hash();
        assert!(h.is_ok());

        //Leaf Node
        let k = Hash::hash_from_bytes(&[47u8; 32].to_vec()).unwrap();
        let vp = vec![[1u8; 32]];
        let mut node2 = Node::<Hash>::new_leaf_node(k, 1, vp.clone());
        let h = node2.node_hash();
        assert!(h.is_ok());
        let h = node2.value_hash();
        assert!(h.is_ok());

        node2.key_preimage = Some([48u8; 32]);
        let b = node2.value();
        let new_node = Node::<Hash>::new_node_from_bytes(b);
        assert!(new_node.is_ok());
        let new_node = new_node.unwrap();
        assert_eq!(node2.node_type, new_node.node_type);
        assert_eq!(node2.node_key, new_node.node_key);
        assert_eq!(node2.value_preimage, new_node.value_preimage);
        assert_eq!(node2.key_preimage, new_node.key_preimage);

        //Empty Node
        let b =Node::<Hash>::new_empty_node().value();
        let new_node = Node::<Hash>::new_node_from_bytes(b);
        assert!(new_node.is_ok());

        let mut node3 = new_node.unwrap();
        let h = node3.node_hash();
        assert!(h.is_ok());
        assert_eq!(h.unwrap(), Hash::hash_zero());
        let h = node3.value_hash();
        assert!(h.is_ok());
        assert_eq!(h.unwrap(), Hash::hash_zero());

        //Bad Size
        let b = vec![];
        let node = Node::<Hash>::new_node_from_bytes(b);
        assert!(node.is_err());
        assert_eq!(node.err().unwrap(), ImplError::ErrNodeBytesBadSize);

        let b = vec![0u8, 1u8, 2u8];
        let node = Node::<Hash>::new_node_from_bytes(b);
        assert!(node.is_err());
        assert_eq!(node.err().unwrap(), ImplError::ErrNodeBytesBadSize);

        let b = vec![NodeTypeLeaf as u8; HASH_BYTE_LEN + 3];
        let node = Node::<Hash>::new_node_from_bytes(b);
        assert!(node.is_err());
        assert_eq!(node.err().unwrap(), ImplError::ErrNodeBytesBadSize);

        let k = Hash::hash_from_bytes(&[47u8; 32].to_vec()).unwrap();
        let vp = vec![[1u8; 32]];
        let valid_node = Node::<Hash>::new_leaf_node(k, 1, vp.clone());
        let b = valid_node.value();
        let node = Node::<Hash>::new_node_from_bytes(b[0..b.len()-32].to_vec());
        assert!(node.is_err());
        assert_eq!(node.err().unwrap(), ImplError::ErrNodeBytesBadSize);

        let k = Hash::hash_from_bytes(&[47u8; 32].to_vec()).unwrap();
        let vp = vec![[1u8; 32]];
        let mut valid_node = Node::<Hash>::new_leaf_node(k, 1, vp.clone());
        valid_node.key_preimage = Some([48u8; 32]);
        let b = valid_node.value();
        let node = Node::<Hash>::new_node_from_bytes(b[0..b.len()-1].to_vec());
        assert!(node.is_err());
        assert_eq!(node.err().unwrap(), ImplError::ErrNodeBytesBadSize);

        //Invalid type
        let b = vec![255u8];
        let node = Node::<Hash>::new_node_from_bytes(b);
        assert!(node.is_err());
        assert_eq!(node.err().unwrap(), ImplError::ErrInvalidNodeFound);
    }

    #[test]
    fn test_node_value_and_data() {
        let a1 = [47u8; 32];
        let a2 = [48u8; 32];
        let a3 = [49u8; 32];
        let mark = [1u8, 1u8, 0u8, 0u8];
        let k = Hash::hash_from_bytes(&a1.to_vec()).unwrap();
        let vp = vec![a2.clone()];

        //Leaf Node
        let mut node = Node::<Hash>::new_leaf_node(k.clone(), 1, vp.clone());
        let mut v = vec![4u8];
        v.append(&mut a1.to_vec());
        v.append(&mut mark.to_vec());
        v.append(&mut a2.to_vec());
        v.push(0);
        assert_eq!(node.canonical_value(), v);

        v.remove(v.len()-1);
        node.key_preimage = Some([49u8; 32]);
        v.push(32u8);
        v.append(&mut a3.to_vec());
        assert_eq!(node.value(), v);

        assert_eq!(node.data().unwrap(), a2.to_vec());

        //Parent Node
        let node = Node::<Hash>::new_parent_node(NodeTypeBranch3, k.clone(), k.clone());
        v = vec![9u8];
        v.append(&mut a1.to_vec());
        v.append(&mut a1.to_vec());
        assert_eq!(node.canonical_value(), v);

        //empty Node
        let node = Node::<Hash>::new_empty_node();
        v = vec![5u8];
        assert_eq!(node.canonical_value(), v);
    }
}