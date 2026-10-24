//! Independent ARMv4T-subset decode into typed IR.
//!
//! Encodings outside the implemented subset are rejected with the original
//! address, width, raw word and a reason. Reserved/newer encodings are never
//! aliased into an ordinary ALU operation.

mod arm;
mod thumb;

pub use arm::decode_arm;
pub use thumb::decode_thumb;

/// Sign-extend the low `bits` of `value` into an `i32`.
fn sign_extend(value: u32, bits: u32) -> i32 {
    let shift = 32 - bits;
    ((value << shift) as i32) >> shift
}

#[cfg(test)]
mod tests;
