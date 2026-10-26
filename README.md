# SidFind

Finds which [HVSC](https://hvsc.c64.org/) tune is used in a C64 `.prg` file (a demo, intro, game, ...).
It is a cross-platform alternative to Wilfred Bos's excellent [SID Known](https://csdb.dk/release/?id=262621).

## Motivation for this project

SID Known by Wilfred Bos is excellent, but it is available for Windows only and is closed source.

Apple is phasing out Rosetta 2, so I will most likely not be able to run SID Known through Wine on my Mac
for much longer. I did not want to set up a virtual machine just to use it, so I gave AI a try and vibe coded
this program for my personal use.

## Important note

**Vibe coded.** It comes with **no warranty of any kind**, and everyone uses it at their own risk.
See [Disclaimer](#disclaimer) and [License](#license).
SID Known is quite more sophisticated than this little project - so if you're on Windows, stick to SID Known instead.

## Download

Prebuilt binaries are attached to each [release](../../releases) (no Rust needed):

| Platform | File |
|---|---|
| Windows (64-bit) | `sidfind-<version>-x86_64-pc-windows-msvc.zip` |
| Linux (64-bit Intel/AMD) | `sidfind-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Linux (64-bit ARM) | `sidfind-<version>-aarch64-unknown-linux-gnu.tar.gz` |
| macOS (Apple silicon) | `sidfind-<version>-aarch64-apple-darwin.tar.gz` |
| macOS (Intel) | `sidfind-<version>-x86_64-apple-darwin.tar.gz` |

Each archive contains the `sidfind` program, this README and the license. `SHA256SUMS.txt` lets you verify the downloads.
SidFind is a command line tool: unpack it and run it from a terminal (see [Usage](#usage)).

The binaries are not code-signed, so your OS may warn about them: on macOS run `xattr -d com.apple.quarantine sidfind`
once if a browser-downloaded copy is blocked, and on Windows choose "More info" and then "Run anyway" in SmartScreen.
Or build it yourself, as described below.


## Usage

You need a copy of HVSC (the folder that contains `DEMOS`, `GAMES` and `MUSICIANS`). Download it from
<https://hvsc.c64.org/downloads>. There are two ways to use it, and you choose.

### Option 1: read HVSC on every run (no setup)

```sh
sidfind --hvsc /path/to/C64Music demo.prg [more.prg ...]
```

This needs no preparation and always uses the HVSC you point it at. It takes about 4 seconds per run on a multi-core
laptop (a run with 16 PRGs took about 7 seconds). Several PRGs in one run share a single pass over HVSC, so that is
faster than running the tool once per file.

### Option 2: build an index once (optional, about 170 MB of disk space)

```sh
sidfind --build-index hvsc.idx --hvsc /path/to/C64Music      # once, about 5 seconds
sidfind --index hvsc.idx demo.prg                            # then: about 0.2 seconds
```

The index holds a sample of the fingerprints of every HVSC tune, plus their paths, so `--hvsc` is not needed when
you use it. Nothing is created unless you ask for it, and you can delete the file at any time to get the disk space
back. Rebuild it after you update HVSC: the index is a snapshot and does not notice changes.

- **Where it is written:** exactly the path you give to `--build-index`; there is no default location and nothing is
  cached automatically. While building, SidFind writes `<file>.tmp` next to it and renames it when done, so a failed
  build never leaves a broken index.
- **Size and accuracy:** `--sampling <N>` (with `--build-index`) trades one for the other. The default of 40 gives about
  170 MB. 64 gives about 105 MB, 32 about 210 MB, and 16 about 400 MB. A tune that shares at least
  `window + N - 1` bytes with the PRG (51 with the default settings) is found, unless that region is mostly zero runs or
  fill bytes, which are ignored. The *score* of weaker matches is estimated from the sample, so a PRG that only partly
  contains a tune can be ranked differently than with option 1. On the 16 example PRGs both options pick the same
  tune. If a result from the index looks doubtful, repeat it with `--hvsc`.
- **Compatibility:** an index written by a different version of SidFind may be rejected. SidFind then says so and asks
  you to build it again.

### Environment variables

Instead of the options you can set `SIDFIND_HVSC` (HVSC folder) or `SIDFIND_INDEX` (index file) once:

```sh
export SIDFIND_INDEX=~/hvsc.idx                  # macOS, Linux
setx SIDFIND_INDEX "C:\C64Music\hvsc.idx"        # Windows (takes effect in new terminals)
```

If several are given, `--index` wins, then `--hvsc`, then `SIDFIND_INDEX`, then `SIDFIND_HVSC`. So an option on the
command line always overrides the environment.

### All options

| Option | Default | Meaning |
|---|---|---|
| `--hvsc <DIR>` | `$SIDFIND_HVSC` | HVSC root folder (option 1) |
| `--index <FILE>` | `$SIDFIND_INDEX` | Use a precomputed index (option 2) |
| `--build-index <FILE>` | | Build an index from `--hvsc` and exit |
| `--sampling <N>` | 40 | With `--build-index`: how sparse the sample is (smaller = bigger and more accurate) |
| `--cycles <N>` | 50000000 | CPU cycles to emulate so a packed PRG can unpack itself (about 50 s of C64 time). `0` skips emulation and matches the file as it is. |
| `--window <N>` | 12 | Match window in bytes. Smaller finds more modified tunes, with more false positives. An index keeps the value it was built with. |
| `--top <N>` | 3 | Candidates to show per PRG |
| `--min-score <N>` | 10 | Hide candidates whose score is below N |

### Reading the output

- **% of tune data found**: how much of the HVSC tune's data was found in the PRG's memory.
- **score**: the number of matching byte windows, each counted as 1 / k², where k is the number of tunes that
  contain the window, multiplied by the share of the tune that was found. Code and tables shared by many tunes (the
  same music player, note tables) therefore count for almost nothing, and so do tunes of which only a small
  part is in the PRG. Compare scores between candidates: a clear winner scores several times higher than the
  runner-up.
- If nothing matches, the tool says why the emulation ended, e.g. `jumped into ROM at $fce2`.

Always check a match manually, as with SID Known.


## Building

SidFind is written in Rust, has no dependencies besides the standard library, and builds the same way on
every platform. You need Rust 1.85 or newer (the project uses the 2024 edition).

### 1. Install Rust

All platforms: install `rustup` from <https://rustup.rs>, then check with `rustc --version`.

| OS | Extra requirements |
|---|---|
| **macOS** | Command line tools: `xcode-select --install` |
| **Linux** | A C linker, e.g. `sudo apt install build-essential` (Debian/Ubuntu), `sudo dnf install gcc` (Fedora) or `sudo pacman -S base-devel` (Arch) |
| **Windows** | rustup asks you to install the Visual Studio C++ Build Tools (the "Desktop development with C++" workload). Alternatively use the `x86_64-pc-windows-gnu` toolchain with MinGW. |

### 2. Build

```sh
git clone <this repository>
cd sidfind
cargo build --release
```

The binary is written to:

| OS | Path |
|---|---|
| macOS, Linux | `target/release/sidfind` |
| Windows | `target\release\sidfind.exe` |

Copy it anywhere on your `PATH`, or run `cargo install --path .` to install it into `~/.cargo/bin`
(`%USERPROFILE%\.cargo\bin` on Windows).

### Building for another OS

The simplest way is to run the steps above on that OS. Cross-compiling works too, but needs a linker for the target
platform, which is the fiddly part:

```sh
rustup target add x86_64-pc-windows-gnu      # example: Windows from Linux/macOS (needs mingw-w64)
cargo build --release --target x86_64-pc-windows-gnu
```

The `cross` tool (<https://github.com/cross-rs/cross>) or a CI matrix on GitHub Actions
(`ubuntu-latest`, `macos-latest`, `windows-latest`) are alternatives that avoid installing linkers yourself.

> Note: the unit tests run automatically on Linux, macOS and Windows (see `.github/workflows/`). The author develops and
> tries SidFind on real HVSC data on macOS only, so Linux and Windows have had less hands-on use.

## How it works

1. The PRG is loaded into a small 6510 emulator (the official opcodes and the stable illegal ones) with 64 KiB of RAM.
   It starts at the BASIC `SYS` target and runs for the cycle budget, or until it hits an idle loop, an unsupported opcode, or a jump into
   ROM. This lets decrunchers unpack the tune. There is no ROM, no interrupt, and no VIC, CIA or SID emulation.
2. Memory is snapshotted every 250,000 cycles and every changed page is kept, because a demo may unpack a tune, use
   it, and later overwrite it.
3. The raw PRG and all snapshots are cut into overlapping windows, and every `.sid` file in HVSC is scanned for them
   with a rolling hash. Windows with fewer than 4 distinct byte values (zero runs, fills) are ignored. Each window is
   reduced to a 40-bit fingerprint. An index keeps only the smallest fingerprint out of every `--sampling` consecutive
   ones ("winnowing"), which is what makes it small while still guaranteeing that any long enough shared region is found.
4. A second kind of window finds tunes that a demo has *relocated*. When code is moved, the 16-bit operands of
   absolute-address instructions (`lda $2174,x`, `jsr $1ac3`, ...) are rewritten, so exact matching fails on
   exactly those bytes. These windows decode the instructions inside the window and treat such operands as
   wildcards. This raised the match of a relocated tune in the test set from 24% to 62% of its bytes.

## Limitations

- Only binary-mode arithmetic is emulated, and of the illegal opcodes only the stable ones (LAX, SAX, SLO, RLA, SRE,
  RRA, DCP, ISC, ANC, ALR, ARR, LXA, SBX, the extra SBC and the NOP variants). A packer using an unstable one (XAA, LAS,
  SHA, SHX, TAS) or a JAM opcode stops early (the output names the opcode). The matcher that finds relocated code still
  treats illegal opcodes as data, so relocated code that uses them is matched on its unmoved bytes only.
- Relocation is handled for absolute-address operands only. Data tables that hold addresses (pointer tables) and
  tunes that the PRG rewrites in other ways can still be ranked weakly. A tune that is only generated after the first
  interrupt can be missed, since interrupts are not emulated. SID Known's approach of hashing the emulated SID register writes can catch such cases; this tool
  does not do that.
- Tunes that are not in HVSC cannot be identified.

## Changelog

### 0.1.3 (2026-10-27)

- The emulator now runs the stable illegal 6502 opcodes (LAX, SAX, SLO, RLA, SRE, RRA, DCP, ISC, ANC, ALR, ARR, LXA,
  SBX, the extra SBC at `$eb`, and the NOP variants). Packers and players that use them used to stop the emulation
  early, so their tune was never unpacked. The unstable opcodes (XAA, LAS, SHA, SHX, TAS) and JAM still stop it.
- A PRG that was saved with load address `$0800` (a BASIC program with its leading zero byte) is now started at its
  `SYS` target, like one at `$0801`. It used to start at the load address and stop on the first byte.
- Tested against 638 intro PRGs whose tune is known: the expected tune is among the top 3 candidates for 98.7% of them
  (630 of 638), up from 97.8% (624) with 0.1.2, using an index with the default settings. The result of each PRG is in
  `2026-10-26_11-12-05_test-results.txt`.
- No change to the index format or to the fingerprints: an index built with 0.1.2 keeps working.

### 0.1.2

- initial release.

## Releasing (maintainers)

Binaries are built by GitHub Actions (`.github/workflows/`). The CI workflow builds and tests on Linux, macOS and
Windows for every push and pull request. To publish a release, bump `version` in `Cargo.toml`, commit, then push a
matching tag:

```sh
git tag v0.1.0
git push origin v0.1.0
```

The release workflow tests and builds all platforms and creates the GitHub release with the archives and
`SHA256SUMS.txt`. It refuses to publish if the tag does not match the version in `Cargo.toml` (`v0.1.0` or `v0.1.0-rc1` for version 0.1.0). A tag with a suffix, such as
`v0.1.0-rc1`, is published as a pre-release.

## Disclaimer

This program was vibe coded by Doc Snyder for personal use. It has not been audited. It has been tried on a set of
~600 C64 intro PRGs with known tunes (see the [changelog](#changelog)), where the right tune was among the
top 3 candidates for 98.7% of them.
Results can be wrong: always check a match manually.

**Everyone uses this program at their own risk. The author takes no responsibility whatsoever for any issues, damage,
loss or other consequences that may result from using it.**

## License

[MIT](LICENSE). Copyright (c) 2026 Doc Snyder.
