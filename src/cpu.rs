//! A minimal C64 "machine": a 6510 CPU (official opcodes, binary-mode arithmetic only) over
//! 64 KiB of RAM with just enough I/O behaviour for decrunchers to run to completion.
//!
//! There is no ROM (calls into it just return), no interrupts, and no VIC/CIA/SID emulation. The goal is not to run a
//! demo but to let its unpacker finish, so the unpacked tune can be found in memory.

use std::fmt;

const C: u8 = 0x01;
const Z: u8 = 0x02;
const I: u8 = 0x04;
const D: u8 = 0x08;
const B: u8 = 0x10;
const U: u8 = 0x20;
const V: u8 = 0x40;
const N: u8 = 0x80;

/// Stack pointer BASIC leaves behind when it SYSes into a program. Some unpackers copy
/// their stub using TSX, so this value matters.
const SYS_STACK_POINTER: u8 = 0xf6;
const CHECKPOINT_CYCLES: u64 = 250_000;
const PAGE: usize = 256;
const CYCLES_PER_RASTER_LINE: u64 = 63;
const RASTER_LINES: u64 = 312;

#[derive(Debug, PartialEq)]
pub enum Stop {
    /// The cycle budget ran out (typically: a main loop waiting for interrupts we don't emulate).
    Budget,
    /// The program jumped to itself, i.e. it is done and idling.
    Idle(u16),
    /// The program jumped into the (absent) KERNAL or BASIC ROM at this address.
    Rom(u16),
    /// An unsupported opcode, or BRK, at this address.
    Halted(u8, u16),
}

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stop::Budget => write!(f, "cycle budget used up"),
            Stop::Idle(pc) => write!(f, "idle loop at ${pc:04x}"),
            Stop::Rom(pc) => write!(f, "jumped into ROM at ${pc:04x}"),
            Stop::Halted(op, pc) => write!(f, "unsupported opcode ${op:02x} at ${pc:04x}"),
        }
    }
}

pub struct Machine {
    pub ram: Box<[u8; 0x10000]>,
    io: [u8; 0x1000],
    a: u8,
    x: u8,
    y: u8,
    sp: u8,
    p: u8,
    pc: u16,
    pub cycles: u64,
    cia_polls: u8,
}

impl Machine {
    /// Loads a PRG and points the CPU at its entry: the BASIC `SYS` target if there is one,
    /// otherwise the load address.
    pub fn from_prg(load: u16, data: &[u8]) -> Machine {
        let mut m = Machine {
            ram: vec![0u8; 0x10000].into_boxed_slice().try_into().unwrap(),
            io: [0; 0x1000],
            a: 0,
            x: 0,
            y: 0,
            sp: SYS_STACK_POINTER,
            p: I | U,
            pc: load,
            cycles: 0,
            cia_polls: 0,
        };
        let end = (load as usize + data.len()).min(0x10000);
        m.ram[load as usize..end].copy_from_slice(&data[..end - load as usize]);
        m.ram[0] = 0x2f;
        m.ram[1] = 0x37;
        if load == 0x0801 {
            if let Some(target) = sys_target(&m.ram[0x0801..]) {
                m.pc = target;
            }
        }
        m
    }

    pub fn run(&mut self, max_cycles: u64) -> Stop {
        while self.cycles < max_cycles {
            let start = self.pc;
            if self.rom_visible(start) {
                // No ROM is loaded. A routine that was JSRed to is stood in for by an RTS;
                // anything else (a JMP, or a fall-through) is the program handing off.
                if !self.entered_by_jsr(start) {
                    return Stop::Rom(start);
                }
                self.cycles += 6;
                let lo = self.pull();
                self.pc = u16::from_le_bytes([lo, self.pull()]).wrapping_add(1);
                continue;
            }
            if let Some(op) = self.step() {
                return Stop::Halted(op, start);
            }
            if self.pc == start {
                return Stop::Idle(start);
            }
        }
        Stop::Budget
    }

    fn rom_visible(&self, addr: u16) -> bool {
        let port = self.ram[1];
        match addr {
            0xa000..=0xbfff => port & 3 == 3,
            0xe000..=0xffff => port & 2 != 0,
            _ => false,
        }
    }

    /// True if the return address on top of the stack follows a `JSR` to `target`.
    fn entered_by_jsr(&self, target: u16) -> bool {
        let at = |off: u8| self.ram[0x100 | self.sp.wrapping_add(off) as usize] as usize;
        let ret = (at(2) << 8 | at(1)) + 1;
        ret >= 3
            && self.ram[ret - 3] == 0x20
            && u16::from_le_bytes([self.ram[ret - 2], self.ram[ret - 1]]) == target
    }

    fn io_visible(&self) -> bool {
        let port = self.ram[1];
        port & 3 != 0 && port & 4 != 0
    }

    fn read(&mut self, addr: u16) -> u8 {
        self.cycles += 1;
        if (0xd000..0xe000).contains(&addr) && self.io_visible() {
            return self.read_io(addr);
        }
        self.ram[addr as usize]
    }

    /// Registers that code polls in wait loops get values that make progress.
    fn read_io(&mut self, addr: u16) -> u8 {
        let raster = (self.cycles / CYCLES_PER_RASTER_LINE) % RASTER_LINES;
        match addr {
            0xd011 => (self.io[0x011] & 0x7f) | ((raster >> 1) as u8 & 0x80),
            0xd012 => raster as u8,
            // keyboard and joystick lines are active low: nothing pressed reads as all ones
            0xdc00 | 0xdc01 => 0xff,
            0xdc0d | 0xdd0d => {
                self.cia_polls = self.cia_polls.wrapping_add(1);
                if self.cia_polls & 1 == 0 { 0x81 } else { 0 }
            }
            _ => self.io[addr as usize - 0xd000],
        }
    }

    fn write(&mut self, addr: u16, v: u8) {
        self.cycles += 1;
        if (0xd000..0xe000).contains(&addr) && self.io_visible() {
            self.io[addr as usize - 0xd000] = v;
        } else {
            self.ram[addr as usize] = v;
        }
    }

    fn fetch(&mut self) -> u8 {
        let v = self.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    fn fetch16(&mut self) -> u16 {
        let lo = self.fetch();
        u16::from_le_bytes([lo, self.fetch()])
    }

    fn read16_zp(&mut self, zp: u8) -> u16 {
        let lo = self.read(zp as u16);
        u16::from_le_bytes([lo, self.read(zp.wrapping_add(1) as u16)])
    }

    fn push(&mut self, v: u8) {
        self.write(0x100 | self.sp as u16, v);
        self.sp = self.sp.wrapping_sub(1);
    }

    fn pull(&mut self) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.read(0x100 | self.sp as u16)
    }

    fn set_nz(&mut self, v: u8) -> u8 {
        self.p = (self.p & !(N | Z)) | (v & N) | if v == 0 { Z } else { 0 };
        v
    }

    fn flag(&self, f: u8) -> bool {
        self.p & f != 0
    }

    fn set_flag(&mut self, f: u8, on: bool) {
        self.p = if on { self.p | f } else { self.p & !f };
    }

    /// Effective address for the addressing mode field `bbb` of the cc=01 group.
    fn addr_alu(&mut self, mode: u8) -> Option<u16> {
        Some(match mode {
            0 => {
                let zp = self.fetch().wrapping_add(self.x);
                self.read16_zp(zp)
            }
            1 => self.fetch() as u16,
            2 => {
                self.pc = self.pc.wrapping_add(1);
                self.pc.wrapping_sub(1)
            }
            3 => self.fetch16(),
            4 => {
                let zp = self.fetch();
                self.read16_zp(zp).wrapping_add(self.y as u16)
            }
            5 => self.fetch().wrapping_add(self.x) as u16,
            6 => self.fetch16().wrapping_add(self.y as u16),
            7 => self.fetch16().wrapping_add(self.x as u16),
            _ => return None,
        })
    }

    /// Effective address for the cc=00 / cc=10 groups; `indexed_by_y` selects LDX/STX's
    /// use of Y in place of X.
    fn addr_rmw(&mut self, mode: u8, indexed_by_y: bool) -> Option<u16> {
        let idx = if indexed_by_y { self.y } else { self.x };
        Some(match mode {
            0 => {
                self.pc = self.pc.wrapping_add(1);
                self.pc.wrapping_sub(1)
            }
            1 => self.fetch() as u16,
            3 => self.fetch16(),
            5 => self.fetch().wrapping_add(idx) as u16,
            7 => self.fetch16().wrapping_add(idx as u16),
            _ => return None,
        })
    }

    fn adc(&mut self, v: u8) {
        let sum = self.a as u16 + v as u16 + (self.p & C) as u16;
        let r = sum as u8;
        self.set_flag(C, sum > 0xff);
        self.set_flag(V, (self.a ^ r) & (v ^ r) & 0x80 != 0);
        self.a = self.set_nz(r);
    }

    fn compare(&mut self, reg: u8, v: u8) {
        self.set_flag(C, reg >= v);
        self.set_nz(reg.wrapping_sub(v));
    }

    fn branch(&mut self, taken: bool) {
        let off = self.fetch() as i8;
        if taken {
            self.cycles += 1;
            self.pc = self.pc.wrapping_add(off as u16);
        }
    }

    /// Shifts/rotates and INC/DEC share one read-modify-write shape.
    fn rmw(&mut self, op: u8, v: u8) -> u8 {
        let carry = self.p & C;
        let r = match op {
            0 => {
                self.set_flag(C, v & 0x80 != 0);
                v << 1
            }
            1 => {
                self.set_flag(C, v & 0x80 != 0);
                v << 1 | carry
            }
            2 => {
                self.set_flag(C, v & 1 != 0);
                v >> 1
            }
            3 => {
                self.set_flag(C, v & 1 != 0);
                v >> 1 | carry << 7
            }
            6 => v.wrapping_sub(1),
            _ => v.wrapping_add(1),
        };
        self.set_nz(r)
    }

    /// Executes one instruction. Returns the opcode if it is not supported.
    fn step(&mut self) -> Option<u8> {
        self.cycles += 1;
        let op = self.fetch();
        match op {
            0x00 => return Some(op),
            0x08 => self.push(self.p | B | U),
            0x18 => self.set_flag(C, false),
            0x20 => {
                let target = self.fetch16();
                let ret = self.pc.wrapping_sub(1);
                self.push((ret >> 8) as u8);
                self.push(ret as u8);
                self.pc = target;
            }
            0x28 => self.p = self.pull() & !B | U,
            0x38 => self.set_flag(C, true),
            0x40 => {
                self.p = self.pull() & !B | U;
                let lo = self.pull();
                self.pc = u16::from_le_bytes([lo, self.pull()]);
            }
            0x48 => self.push(self.a),
            0x4c => self.pc = self.fetch16(),
            0x58 => self.set_flag(I, false),
            0x60 => {
                let lo = self.pull();
                self.pc = u16::from_le_bytes([lo, self.pull()]).wrapping_add(1);
            }
            0x68 => {
                let v = self.pull();
                self.a = self.set_nz(v);
            }
            0x6c => {
                let ptr = self.fetch16();
                let lo = self.read(ptr);
                // the real chip does not carry into the high byte of the pointer
                let hi = self.read(ptr & 0xff00 | (ptr as u8).wrapping_add(1) as u16);
                self.pc = u16::from_le_bytes([lo, hi]);
            }
            0x78 => self.set_flag(I, true),
            0x88 => self.y = self.set_nz(self.y.wrapping_sub(1)),
            0x8a => self.a = self.set_nz(self.x),
            0x98 => self.a = self.set_nz(self.y),
            0x9a => self.sp = self.x,
            0xa8 => self.y = self.set_nz(self.a),
            0xaa => self.x = self.set_nz(self.a),
            0xb8 => self.set_flag(V, false),
            0xba => self.x = self.set_nz(self.sp),
            0xc8 => self.y = self.set_nz(self.y.wrapping_add(1)),
            0xca => self.x = self.set_nz(self.x.wrapping_sub(1)),
            0xd8 => self.set_flag(D, false),
            0xe8 => self.x = self.set_nz(self.x.wrapping_add(1)),
            0xea => {}
            0xf8 => self.set_flag(D, true),
            0x10 => self.branch(!self.flag(N)),
            0x30 => self.branch(self.flag(N)),
            0x50 => self.branch(!self.flag(V)),
            0x70 => self.branch(self.flag(V)),
            0x90 => self.branch(!self.flag(C)),
            0xb0 => self.branch(self.flag(C)),
            0xd0 => self.branch(!self.flag(Z)),
            0xf0 => self.branch(self.flag(Z)),
            _ => return self.step_group(op),
        }
        None
    }

    fn step_group(&mut self, op: u8) -> Option<u8> {
        let (aaa, bbb, cc) = (op >> 5, op >> 2 & 7, op & 3);
        let unsupported = Some(op);
        match cc {
            1 => {
                if aaa == 4 && bbb == 2 {
                    return unsupported; // STA #imm
                }
                let Some(addr) = self.addr_alu(bbb) else { return unsupported };
                if aaa == 4 {
                    self.write(addr, self.a);
                    return None;
                }
                let v = self.read(addr);
                match aaa {
                    0 => self.a = self.set_nz(self.a | v),
                    1 => self.a = self.set_nz(self.a & v),
                    2 => self.a = self.set_nz(self.a ^ v),
                    3 => self.adc(v),
                    5 => self.a = self.set_nz(v),
                    6 => self.compare(self.a, v),
                    _ => self.adc(!v), // SBC
                }
            }
            2 => {
                if bbb == 2 {
                    // accumulator forms; the other bbb=2 opcodes were handled as implied ops
                    if aaa > 3 {
                        return unsupported;
                    }
                    self.a = self.rmw(aaa, self.a);
                    return None;
                }
                if bbb == 0 && aaa != 5 || bbb == 4 || bbb == 6 || aaa == 4 && bbb == 7 {
                    return unsupported;
                }
                let by_y = aaa == 4 || aaa == 5;
                let Some(addr) = self.addr_rmw(bbb, by_y) else { return unsupported };
                match aaa {
                    4 => self.write(addr, self.x),
                    5 => {
                        let v = self.read(addr);
                        self.x = self.set_nz(v);
                    }
                    _ => {
                        let v = self.read(addr);
                        self.write(addr, v);
                        let r = self.rmw(aaa, v);
                        self.write(addr, r);
                    }
                }
            }
            0 => {
                // BIT, STY, LDY, CPY, CPX; JMP/JSR/branches were handled earlier
                let ok = match aaa {
                    1 => bbb == 1 || bbb == 3,
                    4 => bbb == 1 || bbb == 3 || bbb == 5,
                    5 => matches!(bbb, 0 | 1 | 3 | 5 | 7),
                    6 | 7 => matches!(bbb, 0 | 1 | 3),
                    _ => false,
                };
                if !ok {
                    return unsupported;
                }
                let Some(addr) = self.addr_rmw(bbb, false) else { return unsupported };
                if aaa == 4 {
                    self.write(addr, self.y);
                    return None;
                }
                let v = self.read(addr);
                match aaa {
                    1 => {
                        self.set_flag(Z, self.a & v == 0);
                        self.p = self.p & !(N | V) | v & (N | V);
                    }
                    5 => self.y = self.set_nz(v),
                    6 => self.compare(self.y, v),
                    _ => self.compare(self.x, v),
                }
            }
            _ => return unsupported,
        }
        None
    }
}

/// Everything seen while a PRG ran: why it stopped, and every part of memory that changed.
pub struct Run {
    pub stop: Stop,
    pub cycles: u64,
    /// Memory pages that changed between checkpoints, each with the page after it so a
    /// match can straddle the boundary. The tune may be unpacked, used, and then overwritten
    /// by the next part of a demo, so the final memory alone is not enough.
    pub chunks: Vec<Vec<u8>>,
}

pub fn run_prg(load: u16, data: &[u8], max_cycles: u64) -> Run {
    let mut m = Machine::from_prg(load, data);
    let mut seen = vec![0u8; 0x10000];
    let mut chunks = Vec::new();
    loop {
        let limit = (m.cycles + CHECKPOINT_CYCLES).min(max_cycles);
        let stop = m.run(limit);
        for page in (0..0x10000).step_by(PAGE) {
            if m.ram[page..page + PAGE] != seen[page..page + PAGE] {
                seen[page..page + PAGE].copy_from_slice(&m.ram[page..page + PAGE]);
                chunks.push(m.ram[page..(page + 2 * PAGE).min(0x10000)].to_vec());
            }
        }
        if stop != Stop::Budget || m.cycles >= max_cycles {
            return Run { stop, cycles: m.cycles, chunks };
        }
    }
}

/// Finds the address of `SYS nnnn` in a BASIC line (`link link line line $9e ...`).
fn sys_target(basic: &[u8]) -> Option<u16> {
    let rest = basic.get(4..)?;
    let rest = &rest[..rest.len().min(60)];
    let sys = rest.iter().position(|&b| b == 0x9e)?;
    let digits: Vec<u8> = rest[sys + 1..]
        .iter()
        .skip_while(|&&b| b == b' ' || b == b'(')
        .take_while(|b| b.is_ascii_digit())
        .copied()
        .collect();
    std::str::from_utf8(&digits).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(code: &[u8]) -> (Machine, Stop) {
        let mut m = Machine::from_prg(0x1000, code);
        let stop = m.run(100_000);
        (m, stop)
    }

    #[test]
    fn finds_sys_target() {
        let line = [0x0b, 0x08, 0x0a, 0x00, 0x9e, b'2', b'0', b'6', b'1', 0, 0, 0];
        assert_eq!(sys_target(&line), Some(2061));
        let spaced = [0x0d, 0x08, 0xce, 0x07, 0x9e, b' ', b'(', b'2', b'0', b'6', b'4', b')', 0, 0];
        assert_eq!(sys_target(&spaced), Some(2064));
        assert_eq!(sys_target(&[0; 80]), None);
    }

    #[test]
    fn copy_loop_and_idle() {
        // ldx #3; loop: lda $1100,x; sta $1200,x; dex; bpl loop; jmp *
        let mut code = vec![0xa2, 0x03, 0xbd, 0x00, 0x11, 0x9d, 0x00, 0x12, 0xca, 0x10, 0xf7];
        code.extend_from_slice(&[0x4c, 0x0b, 0x10]);
        code.resize(0x100, 0);
        code.extend_from_slice(&[10, 20, 30, 40]);
        let (m, stop) = run(&code);
        assert_eq!(stop, Stop::Idle(0x100b));
        assert_eq!(&m.ram[0x1200..0x1204], &[10, 20, 30, 40]);
    }

    #[test]
    fn arithmetic_flags() {
        // clc; lda #$7f; adc #$01 -> $80, V and N set, C clear
        let (m, _) = run(&[0x18, 0xa9, 0x7f, 0x69, 0x01, 0x4c, 0x05, 0x10]);
        assert_eq!(m.a, 0x80);
        assert!(m.flag(V) && m.flag(N) && !m.flag(C) && !m.flag(Z));
        // sec; lda #$10; sbc #$20 -> $f0, borrow (C clear)
        let (m, _) = run(&[0x38, 0xa9, 0x10, 0xe9, 0x20, 0x4c, 0x05, 0x10]);
        assert_eq!(m.a, 0xf0);
        assert!(!m.flag(C) && m.flag(N));
    }

    #[test]
    fn subroutine_and_stack() {
        // jsr $1006; jmp * ; (1006:) lda #$55; rts
        let (m, stop) = run(&[0x20, 0x06, 0x10, 0x4c, 0x03, 0x10, 0xa9, 0x55, 0x60]);
        assert_eq!((m.a, stop), (0x55, Stop::Idle(0x1003)));
        assert_eq!(m.sp, SYS_STACK_POINTER);
    }

    #[test]
    fn rotates_and_indirect_y() {
        // lda #$81; sec; rol a -> $03 carry set; sta ($fb),y with $fb/$fc = $1300, y=2
        let code = [
            0xa9, 0x00, 0x85, 0xfb, 0xa9, 0x13, 0x85, 0xfc, 0xa0, 0x02, 0xa9, 0x81, 0x38, 0x2a,
            0x91, 0xfb, 0x4c, 0x10, 0x10,
        ];
        let (m, _) = run(&code);
        assert_eq!(m.ram[0x1302], 0x03);
        assert!(m.flag(C));
    }

    #[test]
    fn banking_exposes_io_or_ram() {
        // $01 = $37: write to $d020 goes to I/O; $01 = $34: it lands in RAM
        let (m, _) = run(&[0xa9, 0x05, 0x8d, 0x20, 0xd0, 0xa9, 0x34, 0x85, 0x01, 0xa9, 0x07, 0x8d, 0x20, 0xd0, 0x4c, 0x0e, 0x10]);
        assert_eq!(m.io[0x20], 0x05);
        assert_eq!(m.ram[0xd020], 0x07);
    }

    #[test]
    fn jsr_into_rom_returns_but_jmp_stops() {
        // jsr $e544 (KERNAL clear screen) must come back...
        let (m, stop) = run(&[0x20, 0x44, 0xe5, 0xa9, 0x42, 0x4c, 0x05, 0x10]);
        assert_eq!((m.a, stop), (0x42, Stop::Idle(0x1005)));
        // ...but jmp $e544 is a hand-off
        let (m, stop) = run(&[0x4c, 0x44, 0xe5, 0xa9, 0x42]);
        assert_eq!((m.a, stop), (0, Stop::Rom(0xe544)));
    }

    #[test]
    fn checkpoints_keep_data_that_is_later_overwritten() {
        // lda #$aa; sta $2000; (burn cycles); lda #$bb; sta $2000; jmp *
        let mut code = vec![0xa9, 0xaa, 0x8d, 0x00, 0x20, 0xa2, 0xff, 0xa0, 0xff];
        // inner loop: dey; bne; dex; bne = about 1300 cycles per outer pass, ~330k in total
        code.extend_from_slice(&[0x88, 0xd0, 0xfd, 0xca, 0xd0, 0xfa]);
        code.extend_from_slice(&[0xa9, 0xbb, 0x8d, 0x00, 0x20, 0x4c, 0x14, 0x10]);
        let run = run_prg(0x1000, &code, 10_000_000);
        assert_eq!(run.stop, Stop::Idle(0x1014));
        let has = |v: u8| run.chunks.iter().any(|c| c.starts_with(&[v]) && c.len() >= 256);
        assert!(has(0xaa) && has(0xbb));
    }

    #[test]
    fn nothing_is_pressed_on_the_joystick_port() {
        // wait: lda $dc01; and #$10; bne wait would fall through at once if fire read as pressed
        let (_, stop) = run(&[0xad, 0x01, 0xdc, 0x29, 0x10, 0xd0, 0xf9, 0x4c, 0x07, 0x10]);
        assert_eq!(stop, Stop::Budget);
    }

    #[test]
    fn stops_on_unsupported_opcode() {
        let (_, stop) = run(&[0xea, 0x02]);
        assert_eq!(stop, Stop::Halted(0x02, 0x1001));
    }
}
