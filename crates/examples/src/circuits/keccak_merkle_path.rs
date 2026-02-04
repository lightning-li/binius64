// Copyright 2025 Irreducible Inc.

//! Keccak-256 Merkle path verification circuit.
//!
//! This circuit verifies the correctness of a Merkle path to a root,
//! where the Merkle tree uses Keccak-256 hash function:
//! `parent = keccak256(left_child || right_child)`.

use anyhow::{Result, bail};
use binius_circuits::keccak::fixed_length::keccak256;
use binius_core::word::Word;
use binius_frontend::{CircuitBuilder, Wire, WitnessFiller};
use clap::Args;
use sha3::Digest;

use crate::ExampleCircuit;

/// Number of 64-bit words in a 256-bit hash digest.
const N_WORDS_PER_HASH: usize = 4;
/// Number of bytes in a 256-bit hash digest.
const HASH_BYTES: usize = 32;

/// Represents which side a sibling is on in the Merkle path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiblingSide {
	/// Sibling is on the left: hash(sibling || current)
	Left,
	/// Sibling is on the right: hash(current || sibling)
	Right,
}

/// Keccak Merkle path verification circuit.
///
/// Verifies that a leaf hashes up to a given root through a series of
/// Keccak-256 hash operations along a Merkle path.
pub struct KeccakMerklePathExample {
	/// Maximum depth of the Merkle tree (number of levels from leaf to root).
	max_depth: usize,
	/// Wires for the leaf hash (4 words × 64 bits = 256 bits).
	leaf: [Wire; N_WORDS_PER_HASH],
	/// Wires for each sibling hash along the path.
	siblings: Vec<[Wire; N_WORDS_PER_HASH]>,
	/// Wires indicating the side of each sibling (0 = left, 1 = right).
	sides: Vec<Wire>,
	/// Wire for the expected root hash.
	root: [Wire; N_WORDS_PER_HASH],
	/// Wire for the actual path length (must be <= max_depth).
	path_length: Wire,
}

/// Circuit parameters for Keccak Merkle path verification.
#[derive(Args, Debug, Clone)]
pub struct Params {
	/// Maximum depth of the Merkle tree (number of hash levels from leaf to root).
	/// For a tree with 2^n leaves, depth = n.
	#[arg(long, default_value = "30")]
	pub max_depth: usize,
}

/// Instance data for a specific Merkle path verification.
#[derive(Args, Debug, Clone)]
pub struct Instance {
	/// The leaf hash as a hex string (32 bytes / 64 hex chars).
	#[arg(long)]
	pub leaf: Option<String>,

	/// The sibling hashes along the path, as comma-separated hex strings.
	/// Each hash is 32 bytes / 64 hex chars.
	#[arg(long, value_delimiter = ',')]
	pub siblings: Option<Vec<String>>,

	/// The sides of siblings along the path, as comma-separated values.
	/// Use 'L' or 'l' for left, 'R' or 'r' for right.
	#[arg(long, value_delimiter = ',')]
	pub sides: Option<Vec<String>>,

	/// The expected root hash as a hex string (32 bytes / 64 hex chars).
	#[arg(long)]
	pub root: Option<String>,

	/// Use random test data with this path length (for testing).
	#[arg(long)]
	pub random_depth: Option<usize>,
}

impl ExampleCircuit for KeccakMerklePathExample {
	type Params = Params;
	type Instance = Instance;

	fn build(params: Params, builder: &mut CircuitBuilder) -> Result<Self> {
		let max_depth = params.max_depth;

		if max_depth == 0 {
			bail!("max_depth must be at least 1");
		}

		// Create input wires for the leaf hash
		let leaf: [Wire; N_WORDS_PER_HASH] = std::array::from_fn(|_| builder.add_inout());

		// Create wires for siblings and their sides
		let mut siblings = Vec::with_capacity(max_depth);
		let mut sides = Vec::with_capacity(max_depth);

		for _ in 0..max_depth {
			siblings.push(std::array::from_fn(|_| builder.add_inout()));
			sides.push(builder.add_inout());
		}

		// Create wire for the expected root
		let root: [Wire; N_WORDS_PER_HASH] = std::array::from_fn(|_| builder.add_inout());

		// Create wire for path length
		let path_length = builder.add_inout();

		// Build the Merkle path verification circuit
		let mut current = leaf;

		for i in 0..max_depth {
			let sibling = siblings[i];
			let side = sides[i];

			// Compute the next hash: if side == 0 (left), hash(sibling || current)
			//                        if side == 1 (right), hash(current || sibling)

			// Build message: [first_hash (4 words), second_hash (4 words)]
			// Total: 8 words = 64 bytes
			let message: Vec<Wire> = (0..8)
				.map(|j| {
					let word_idx = j % N_WORDS_PER_HASH;
					if j < N_WORDS_PER_HASH {
						// First 4 words: select between current and sibling based on side
						// side == 0 (left sibling): first = sibling, second = current
						// side == 1 (right sibling): first = current, second = sibling
						builder.select(side, current[word_idx], sibling[word_idx])
					} else {
						// Last 4 words: the other one
						builder.select(side, sibling[word_idx], current[word_idx])
					}
				})
				.collect();

			// Compute keccak256(message) where message is 64 bytes
			let hash_result = keccak256(builder, &message, 64);

			// Check if we've exceeded the path length
			let current_idx = builder.add_constant_64(i as u64);
			let past_length = builder.icmp_uge(current_idx, path_length);

			// If past the path length, keep the current value; otherwise use the hash result
			current = std::array::from_fn(|j| builder.select(past_length, current[j], hash_result[j]));
		}

		// Assert that the final computed hash equals the expected root
		builder.assert_eq_v("computed root equals expected root", root, current);

		Ok(Self {
			max_depth,
			leaf,
			siblings,
			sides,
			root,
			path_length,
		})
	}

	fn populate_witness(&self, instance: Instance, filler: &mut WitnessFiller) -> Result<()> {
		let (leaf_bytes, siblings_data, root_bytes, path_len) = if let Some(random_depth) =
			instance.random_depth
		{
			// Generate random test data
			generate_random_merkle_path(random_depth, self.max_depth)?
		} else {
			// Parse provided data
			let leaf_bytes = parse_hex_hash(
				instance
					.leaf
					.as_deref()
					.unwrap_or("0000000000000000000000000000000000000000000000000000000000000000"),
			)?;

			let siblings_hex = instance.siblings.unwrap_or_default();
			let sides_str = instance.sides.unwrap_or_default();

			if siblings_hex.len() != sides_str.len() {
				bail!(
					"Number of siblings ({}) must match number of sides ({})",
					siblings_hex.len(),
					sides_str.len()
				);
			}

			if siblings_hex.len() > self.max_depth {
				bail!(
					"Path length ({}) exceeds max_depth ({})",
					siblings_hex.len(),
					self.max_depth
				);
			}

			let mut siblings_data = Vec::with_capacity(siblings_hex.len());
			for (sib_hex, side_str) in siblings_hex.iter().zip(sides_str.iter()) {
				let sib_bytes = parse_hex_hash(sib_hex)?;
				let side = match side_str.to_uppercase().as_str() {
					"L" | "LEFT" => SiblingSide::Left,
					"R" | "RIGHT" => SiblingSide::Right,
					_ => bail!("Invalid side '{}', expected 'L' or 'R'", side_str),
				};
				siblings_data.push((sib_bytes, side));
			}

			// Compute the root by following the path
			let root_bytes = if instance.root.is_some() {
				parse_hex_hash(instance.root.as_deref().unwrap())?
			} else {
				compute_merkle_root(&leaf_bytes, &siblings_data)
			};

			let path_len = siblings_data.len();
			(leaf_bytes, siblings_data, root_bytes, path_len)
		};

		// Populate leaf
		populate_hash_wires(filler, &self.leaf, &leaf_bytes);

		// Populate siblings and sides
		for i in 0..self.max_depth {
			if i < siblings_data.len() {
				let (sib_bytes, side) = &siblings_data[i];
				populate_hash_wires(filler, &self.siblings[i], sib_bytes);
				filler[self.sides[i]] = match side {
					SiblingSide::Left => Word::ZERO,
					SiblingSide::Right => Word::ALL_ONE,
				};
			} else {
				// Pad with zeros for unused path elements
				populate_hash_wires(filler, &self.siblings[i], &[0u8; HASH_BYTES]);
				filler[self.sides[i]] = Word::ZERO;
			}
		}

		// Populate root
		populate_hash_wires(filler, &self.root, &root_bytes);

		// Populate path length
		filler[self.path_length] = Word(path_len as u64);

		Ok(())
	}

	fn param_summary(params: &Self::Params) -> Option<String> {
		Some(format!("d{}", params.max_depth))
	}
}

/// Parse a hex string into a 32-byte hash.
fn parse_hex_hash(hex_str: &str) -> Result<[u8; HASH_BYTES]> {
	let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
	if hex_str.len() != 64 {
		bail!(
			"Hash hex string must be 64 characters (32 bytes), got {}",
			hex_str.len()
		);
	}
	let bytes = hex::decode(hex_str)?;
	Ok(bytes.try_into().unwrap())
}

/// Populate wire values from a 32-byte hash.
fn populate_hash_wires(filler: &mut WitnessFiller, wires: &[Wire; N_WORDS_PER_HASH], hash: &[u8; HASH_BYTES]) {
	for (i, chunk) in hash.chunks(8).enumerate() {
		let word = u64::from_le_bytes(chunk.try_into().unwrap());
		filler[wires[i]] = Word(word);
	}
}

/// Compute the Merkle root by hashing the leaf up through the path.
fn compute_merkle_root(leaf: &[u8; HASH_BYTES], path: &[([u8; HASH_BYTES], SiblingSide)]) -> [u8; HASH_BYTES] {
	let mut current = *leaf;

	for (sibling, side) in path {
		let message = match side {
			SiblingSide::Left => [sibling.as_slice(), current.as_slice()].concat(),
			SiblingSide::Right => [current.as_slice(), sibling.as_slice()].concat(),
		};

		let mut hasher = sha3::Keccak256::new();
		hasher.update(&message);
		current = hasher.finalize().into();
	}

	current
}

/// Generate random test data for a Merkle path.
fn generate_random_merkle_path(
	depth: usize,
	max_depth: usize,
) -> Result<([u8; HASH_BYTES], Vec<([u8; HASH_BYTES], SiblingSide)>, [u8; HASH_BYTES], usize)> {
	use rand::{Rng, SeedableRng};

	if depth > max_depth {
		bail!("random_depth ({}) exceeds max_depth ({})", depth, max_depth);
	}

	let mut rng = rand::rngs::StdRng::seed_from_u64(42);

	// Generate random leaf
	let mut leaf = [0u8; HASH_BYTES];
	rng.fill(&mut leaf);

	// Generate random siblings and sides
	let mut siblings_data = Vec::with_capacity(depth);
	for _ in 0..depth {
		let mut sibling = [0u8; HASH_BYTES];
		rng.fill(&mut sibling);
		let side = if rng.random_bool(0.5) {
			SiblingSide::Left
		} else {
			SiblingSide::Right
		};
		siblings_data.push((sibling, side));
	}

	// Compute the root
	let root = compute_merkle_root(&leaf, &siblings_data);

	Ok((leaf, siblings_data, root, depth))
}

#[cfg(test)]
mod tests {
	use binius_core::verify::verify_constraints;
	use binius_frontend::CircuitBuilder;

	use super::*;

	#[test]
	fn test_merkle_path_depth_1() {
		test_merkle_path_with_random_depth(1, 5);
	}

	#[test]
	fn test_merkle_path_depth_3() {
		test_merkle_path_with_random_depth(3, 5);
	}

	#[test]
	fn test_merkle_path_depth_5() {
		test_merkle_path_with_random_depth(5, 5);
	}

	#[test]
	fn test_merkle_path_depth_10() {
		test_merkle_path_with_random_depth(10, 10);
	}

	fn test_merkle_path_with_random_depth(depth: usize, max_depth: usize) {
		let params = Params { max_depth };
		let instance = Instance {
			leaf: None,
			siblings: None,
			sides: None,
			root: None,
			random_depth: Some(depth),
		};

		// Build circuit
		let mut builder = CircuitBuilder::new();
		let example = KeccakMerklePathExample::build(params, &mut builder).unwrap();
		let circuit = builder.build();

		// Populate witness
		let mut filler = circuit.new_witness_filler();
		example.populate_witness(instance, &mut filler).unwrap();

		// Verify wire witness
		circuit.populate_wire_witness(&mut filler).unwrap();

		// Verify constraints
		let cs = circuit.constraint_system();
		verify_constraints(cs, &filler.into_value_vec()).expect("Constraints should be satisfied");
	}

	#[test]
	fn test_compute_merkle_root() {
		// Test with known values
		let leaf = [0u8; 32];
		let sibling = [1u8; 32];

		// Hash(sibling || leaf) where sibling is on the left
		let path = vec![(sibling, SiblingSide::Left)];
		let root = compute_merkle_root(&leaf, &path);

		// Verify by computing manually
		let mut hasher = sha3::Keccak256::new();
		hasher.update(&sibling);
		hasher.update(&leaf);
		let expected: [u8; 32] = hasher.finalize().into();

		assert_eq!(root, expected);
	}
}
