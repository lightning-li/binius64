// Copyright 2025 Irreducible Inc.

use anyhow::Result;
use binius_examples::{Cli, circuits::keccak_merkle_path::KeccakMerklePathExample};

fn main() -> Result<()> {
	Cli::<KeccakMerklePathExample>::new("keccak_merkle_path")
		.about("Keccak-256 Merkle path verification circuit example")
		.run()
}
