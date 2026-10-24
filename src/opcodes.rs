//! Instruction lengths of the official 6502 opcodes, shared with the matcher so it can tell
//! which bytes are absolute addresses (the bytes a relocator rewrites).

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Info {
    pub len: usize,
    /// Operand is a 16-bit address (absolute, absolute indexed, indirect, JSR/JMP).
    pub abs: bool,
}

const ONE: Info = Info { len: 1, abs: false };
const TWO: Info = Info { len: 2, abs: false };
/// How the matcher steps over a byte that is not a supported opcode (most likely data).
pub const DATA: Info = ONE;
const ABS: Info = Info { len: 3, abs: true };

/// `None` for opcodes the emulator does not support (illegal opcodes, BRK).
pub fn info(op: u8) -> Option<Info> {
    let (aaa, bbb) = (op >> 5, op >> 2 & 7);
    match op {
        0x00 => None,
        0x20 | 0x4c | 0x6c => Some(ABS),
        0x08 | 0x18 | 0x28 | 0x38 | 0x40 | 0x48 | 0x58 | 0x60 | 0x68 | 0x78 | 0x88 | 0x8a
        | 0x98 | 0x9a | 0xa8 | 0xaa | 0xb8 | 0xba | 0xc8 | 0xca | 0xd8 | 0xe8 | 0xea | 0xf8 => {
            Some(ONE)
        }
        _ if op & 0x1f == 0x10 => Some(TWO), // branches
        _ => match op & 3 {
            1 if aaa == 4 && bbb == 2 => None, // STA #imm
            1 => Some(by_mode(bbb)),
            2 if bbb == 2 => (aaa <= 3).then_some(ONE), // ASL/ROL/LSR/ROR A
            2 if bbb == 0 && aaa != 5 || bbb == 4 || bbb == 6 || aaa == 4 && bbb == 7 => None,
            2 => Some(by_mode(bbb)),
            0 => {
                let ok = match aaa {
                    1 => bbb == 1 || bbb == 3,
                    4 => bbb == 1 || bbb == 3 || bbb == 5,
                    5 => matches!(bbb, 0 | 1 | 3 | 5 | 7),
                    6 | 7 => matches!(bbb, 0 | 1 | 3),
                    _ => false,
                };
                ok.then(|| by_mode(bbb))
            }
            _ => None,
        },
    }
}

/// Addressing mode field `bbb` (the same numbering for all three opcode groups).
fn by_mode(bbb: u8) -> Info {
    match bbb {
        3 | 6 | 7 => ABS,
        _ => TWO,
    }
}
