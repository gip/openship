use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use swc_core::ecma::ast::Program;

use crate::graph::Mangled;

// Abstract Hash
#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize)]
pub struct AbsHash(pub String);

#[derive(PartialEq, Eq, Clone, Hash, Debug, Serialize, Deserialize)]
pub struct ImplHash(pub String);

fn u64_to_hash(num: u64) -> String {
    const ALPHABET: &str = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const BASE: u64 = ALPHABET.len() as u64;
    const MAX_LENGTH: usize = 11;

    // Scrambling constants
    const MULT: u64 = 0xc3326ad887ae7811; // Large prime multiplier
    const XOR: u64 = 0x7edd869db2c3af1f; // Large prime XOR value

    // Scramble the input
    let mut n = num.wrapping_mul(MULT).wrapping_add(1);
    n ^= n >> 30;
    n = n.wrapping_mul(MULT);
    n ^= n >> 27;
    n = n.wrapping_mul(MULT);
    n ^= n >> 31;
    n ^= XOR;

    let mut result = vec!['0'; MAX_LENGTH];

    for i in (0..MAX_LENGTH).rev() {
        let index = (n % BASE) as usize;
        result[i] = ALPHABET.chars().nth(index).unwrap();
        n /= BASE;
    }

    result.into_iter().collect()
}

pub fn program_hash(program: &Program) -> AbsHash {
    let mut hasher = DefaultHasher::new();
    program.hash(&mut hasher);
    let hash = u64_to_hash(hasher.finish());
    AbsHash(format!("osha_1{hash}"))
}

pub fn program_impl_hash(abs_hash: &AbsHash, deps: HashMap<Mangled, ImplHash>) -> ImplHash {
    let mut hasher = DefaultHasher::new();
    let AbsHash(abs_hash_str) = abs_hash;
    abs_hash_str.hash(&mut hasher);
    for (key, value) in deps.iter() {
        key.hash(&mut hasher);
        value.hash(&mut hasher);
    }
    let hash = u64_to_hash(hasher.finish());
    ImplHash(format!("oshi_1{hash}"))
}

pub fn depencency_hash(name: &str, version: &str) -> (AbsHash, ImplHash) {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    version.hash(&mut hasher);
    let hash = u64_to_hash(hasher.finish());
    (
        AbsHash(format!("osha_1{hash}")),
        ImplHash(format!("oshi_1{hash}")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use swc_core::common::{BytePos, Span};
    use swc_core::ecma::ast::Module;

    #[test]

    fn test_hash() {
        let span = Span {
            hi: BytePos(1),
            lo: BytePos(0),
        };
        let program = Program::Module(Module {
            span: span,
            body: vec![],
            shebang: None,
        });
        assert_eq!(u64_to_hash(0), "8GxynqChlO7");
        assert_eq!(
            program_hash(&program),
            AbsHash("osha_1ixxfAnWr3K4".to_string())
        );
    }
}
