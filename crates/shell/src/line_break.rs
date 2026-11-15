//! Where a line may be broken, the way Qt decides it: a port of `getLineBreaks` and of the
//! Thai, Myanmar, Indic, Tibetan and Khmer tailoring in qtbase
//! `src/corelib/text/qunicodetools.cpp` (Qt 6.12, Unicode 17). Qt does not run the rules of
//! UAX #14 one by one: it keeps a pair table and state machines beside it for the rules a pair
//! cannot tell (LB15a to LB15c, LB19a, LB20a, LB21a, LB25, LB28a). Names and order follow
//! that file.

use std::ffi::{c_int, c_uchar, c_void};
use std::sync::OnceLock;

use crate::line_break_data::RANGES;

// `QUnicodeTables::LineBreakClass` by its numbers, which the code compares classes by and
// `TABLE` is indexed with; the classes the code never names are left out.
const OP: u8 = 0;
const CL: u8 = 1;
const CP: u8 = 2;
const QU: u8 = 3;
const QU_PI: u8 = 4;
const QU_PF: u8 = 5;
const QU_19: u8 = 6;
const GL: u8 = 7;
const EX: u8 = 9;
const SY: u8 = 10;
const IS: u8 = 11;
const PR: u8 = 12;
const PO: u8 = 13;
const NU: u8 = 14;
const AL: u8 = 15;
const HL: u8 = 16;
const ID: u8 = 17;
const HY: u8 = 19;
const WS_HY: u8 = 20;
const HH: u8 = 21;
const WS_BA: u8 = 23;
const HYBA: u8 = 24;
const ZW: u8 = 27;
const CM: u8 = 28;
const WJ: u8 = 29;
const H2: u8 = 30;
const JT: u8 = 34;
const RI: u8 = 35;
const CB: u8 = 36;
const EM: u8 = 38;
const AK: u8 = 39;
const AP: u8 = 40;
const AS: u8 = 41;
const VI: u8 = 42;
const VF: u8 = 43;
const ZWJ: u8 = 44;
const SA: u8 = 45;
const SP: u8 = 47;
const CR: u8 = 48;
const LF: u8 = 49;
const BK: u8 = 50;

/// What the code reads of `QUnicodeTables::Properties`.
#[derive(Clone, Copy)]
struct Props {
    /// The class Qt's tables give the character (qtbase `util/unicode/main.cpp`,
    /// `initLineBreak`): AI and XX are AL, NL is BK, CJ is NS.
    class: u8,
    flags: u8,
}

impl Props {
    const LETTER: Props = Props {
        class: AL,
        flags: 0,
    };
    fn initial_quote(self) -> bool {
        self.flags & 1 != 0
    }
    fn final_quote(self) -> bool {
        self.flags & 2 != 0
    }
    fn mark(self) -> bool {
        self.flags & 4 != 0
    }
    /// East Asian Width is W, F or H.
    fn east_asian(self) -> bool {
        self.flags & 8 != 0
    }
    fn unassigned_pictographic(self) -> bool {
        self.flags & 16 != 0
    }
}

fn props(c: char) -> Props {
    let at = RANGES.partition_point(|&(first, _, _)| first <= c as u32) - 1;
    let (_, class, flags) = RANGES[at];
    Props { class, flags }
}

fn class(c: char) -> u8 {
    props(c).class
}

/// `LB::NS`: the number sequences of LB25.
mod number {
    pub const NONE: u8 = 0;
    pub const START: u8 = 1;
    pub const CONTINUE: u8 = 2;
    pub const BREAK: u8 = 3;
    pub const NEED_OP_NU: u8 = 4;
    pub const C_NEED_NU: u8 = 5;
    pub const C_NEED_IS_NU: u8 = 6;

    pub const XX: usize = 0;
    pub const PRPO: usize = 1;
    pub const OP: usize = 2;
    pub const HY: usize = 3;
    pub const NU: usize = 4;
    pub const SY: usize = 5;
    pub const IS: usize = 6;
    pub const CLCP: usize = 7;

    const N: u8 = NONE;
    const S: u8 = START;
    const C: u8 = CONTINUE;
    const B: u8 = BREAK;
    const O: u8 = NEED_OP_NU;
    #[rustfmt::skip]
    pub const ACTION: [[u8; 8]; 8] = [
        // XX PRPO OP HY NU SY IS CLCP
        [N, O, S, N, S, N, N, N], // XX
        [N, O, C, B, S, N, N, N], // PRPO
        [N, S, S, B, C, N, C, N], // OP
        [N, N, N, S, C, N, N, N], // HY
        [B, B, B, B, C, C, C, C], // NU
        [B, B, B, B, C, C, C, C], // SY
        [B, B, B, B, C, C, C, C], // IS
        [B, C, B, B, B, B, B, B], // CLCP
    ];

    pub fn class(cls: u8) -> usize {
        match cls {
            super::PR | super::PO => PRPO,
            super::OP => OP,
            super::HY => HY,
            super::NU => NU,
            super::SY => SY,
            super::IS => IS,
            super::CL | super::CP => CLCP,
            _ => XX,
        }
    }
}

/// `LB::BRS`: the orthographic syllables of Brahmic scripts, LB28a.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Brahmic {
    None,
    Start,
    VF2,
    VI2,
    VIAK3,
    Four,
    FourVF,
    Restart,
}

const DOTTED_CIRCLE: char = '\u{25cc}';

fn brahmic(state: Brahmic, cls: u8, c: char) -> Brahmic {
    if cls == CM {
        return state;
    }
    match state {
        Brahmic::Start => {
            if cls == VF {
                return Brahmic::VF2;
            }
            if cls == VI {
                return Brahmic::VI2;
            }
            if c == DOTTED_CIRCLE || cls == AK || cls == AS {
                return Brahmic::Four;
            }
        }
        Brahmic::VI2 => {
            if c == DOTTED_CIRCLE || cls == AK {
                return Brahmic::VIAK3;
            }
        }
        Brahmic::Four => {
            if cls == VF {
                return Brahmic::FourVF;
            }
            return Brahmic::Restart;
        }
        Brahmic::None => {
            if c == DOTTED_CIRCLE || cls == AK || cls == AS {
                return Brahmic::Start;
            }
        }
        Brahmic::VF2 | Brahmic::FourVF | Brahmic::VIAK3 | Brahmic::Restart => {}
    }
    Brahmic::None
}

// `LB::Action`.
const PB: u8 = 0;
const DB: u8 = 1;
const IB: u8 = 2;
const CI: u8 = 3;
const CPB: u8 = 4;
const HHB: u8 = 5;
const INB: u8 = 6;
const DN: u8 = 7;

/// `LB::breakTable`: the action between a class (row) and the one after it (column).
#[rustfmt::skip]
const TABLE: [[u8; 44]; 44] = {
    const CP: u8 = CPB;
    const HH: u8 = HHB;
    const IN: u8 = INB;
    [
        [PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, CP, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB], // OP
        [DB, PB, PB, IB, IB, PB, IB, IB, PB, PB, PB, PB, DB, DB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // CL
        [DB, PB, PB, IB, IB, PB, IB, IB, PB, PB, PB, PB, DB, DB, IB, IB, IB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // CP
        [IB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, IB, IB, IB, IB, IB, DB, IB, IB, IB, IB, IB, IB, IB, IB, IB, PB, CI, PB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB], // QU
        [PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, CP, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB], // QuPi
        [IB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, PB, CI, PB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB], // QuPf
        [IB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, PB, CI, PB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB], // Qu19
        [IB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, PB, CI, PB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB], // GL
        [DB, PB, PB, DB, IB, PB, IB, IB, IB, PB, PB, PB, DB, DB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // NS
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, DB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // EX
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, DB, DB, DB, IB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // SY
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DN, DB, IB, IB, IB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // IS
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, DB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, IB, IB, IB, IB, IB, DB, DB, IB, IB, DB, DB, DB, DB, DB], // PR
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, DB, IB, IB, IB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // PO
        [IN, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, IB, IB, IB, IB, IB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // NU
        [IN, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, IB, IB, IB, IB, IB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // AL
        [IN, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, IB, IB, IB, IB, IB, DB, IB, IB, CI, CI, IB, CI, CI, DB, DB, PB, CI, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // HL
        [DB, PB, PB, DB, IB, PB, IB, IB, IB, PB, PB, PB, DB, IB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // ID
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, DB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // IN
        [HH, PB, PB, IB, IB, PB, IB, HH, IB, PB, PB, PB, HH, HH, IB, HH, HH, HH, IB, IB, IB, IB, IB, IB, IB, HH, HH, PB, CI, PB, HH, HH, HH, HH, HH, HH, DB, DB, DB, HH, HH, HH, HH, HH], // HY
        [HH, PB, PB, IB, IB, PB, IB, HH, IB, PB, PB, PB, HH, HH, IB, IB, IB, HH, IB, IB, IB, IB, IB, IB, IB, HH, HH, PB, CI, PB, HH, HH, HH, HH, HH, HH, DB, DB, DB, HH, HH, HH, HH, HH], // WsHy
        [HH, PB, PB, IB, IB, PB, IB, HH, IB, PB, PB, PB, HH, HH, IB, HH, HH, HH, IB, IB, IB, IB, IB, IB, IB, HH, HH, PB, CI, PB, HH, HH, HH, HH, HH, HH, DB, DB, DB, HH, HH, HH, HH, HH], // HH
        [HH, PB, PB, IB, IB, PB, IB, HH, IB, PB, PB, PB, HH, HH, HH, HH, HH, HH, IB, IB, IB, IB, IB, IB, IB, HH, HH, PB, CI, PB, HH, HH, HH, HH, HH, HH, DB, DB, DB, HH, HH, HH, HH, HH], // BA
        [HH, PB, PB, IB, IB, PB, IB, HH, IB, PB, PB, PB, HH, HH, HH, IB, IB, HH, IB, IB, IB, IB, IB, IB, IB, HH, HH, PB, CI, PB, HH, HH, HH, HH, HH, HH, DB, DB, DB, HH, HH, HH, HH, HH], // WsBa
        [PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, DB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB, PB], // HyBa
        [IB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, PB, CI, PB, IB, IB, IB, IB, IB, IB, DB, IB, IB, IB, IB, IB, IB, IB], // BB
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, DB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, PB, PB, CI, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // B2
        [DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // ZW
        [IB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, DB, IB, IB, IB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // CM
        [IB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, PB, CI, PB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB, IB], // WJ
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, IB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, IB, IB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // H2
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, IB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, DB, IB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // H3
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, IB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, IB, IB, IB, IB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // JL
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, IB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, IB, IB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // JV
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, IB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, DB, IB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // JT
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, DB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, DB, DB, IB, DB, DB, DB, DB, DB, DB, DB, DB], // RI
        [DB, PB, PB, IB, IB, PB, IB, IB, DB, PB, PB, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, PB, CI, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // CB
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, IB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, DB, DB, DB, DB, DB, IB, DB, DB, DB, DB, DB], // EB
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, IB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, CI, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // EM
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, DB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, DB, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, IB, IB], // AK
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, DB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, DB, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, IB, DB, IB, DB, DB], // AP
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, DB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, DB, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, IB, IB], // AS
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, DB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, DB, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // VI
        [DB, PB, PB, IB, IB, PB, IB, IB, IB, PB, PB, PB, DB, DB, DB, DB, DB, DB, IB, IB, IB, IB, IB, IB, IB, DB, DB, PB, DB, PB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB, DB], // VF
    ]
};

/// Whether a line may be broken before each character of `text`, per byte offset, and at its
/// end: the `getLineBreaks` of `QUnicodeTools::initCharAttributes` as text layout calls it
/// (QTextEngine::attributes), with `HangulLineBreakTailoring` (Korean breaks at spaces, not
/// between syllables). `tailor` then goes over them item by item.
pub fn line_breaks(text: &str) -> Vec<bool> {
    pair_breaks(text)
}

/// `getLineBreaks`.
fn pair_breaks(text: &str) -> Vec<bool> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let len = chars.len();
    let mut at = vec![false; len + 1];

    let mut nestart = 0usize;
    let mut nelast = number::XX;
    let mut neactlast = number::NONE;
    let mut brs = Brahmic::None;
    let mut brs_start = 0usize;
    let mut lcls = LF;
    let mut cls = lcls;
    let mut last = props('\n');

    #[derive(PartialEq)]
    enum To {
        Next,
        NoClsUpdate,
    }

    for (i, &(_, c)) in chars.iter().enumerate() {
        let mut prop = props(c);
        let mut ncls = prop.class;
        if (H2..=JT).contains(&ncls) || (('\u{3130}'..='\u{318f}').contains(&c) && ncls == ID) {
            // LB27: Korean uses spaces for line breaking.
            ncls = AL;
        }
        if ncls == SA && prop.mark() {
            // LB1
            ncls = CM;
        }
        let next_class = chars.get(i + 1).map(|&(_, n)| class(n));

        let to = 'body: {
            if ncls == QU {
                if prop.initial_quote() {
                    // LB15a
                    if [BK, CR, LF, OP, QU, QU_PI, QU_PF, GL, SP, ZW].contains(&lcls) {
                        ncls = QU_PI;
                    }
                } else if prop.final_quote() {
                    // LB15b
                    let nncls = next_class.unwrap_or(LF);
                    if [
                        SP, GL, WJ, CL, QU, QU_PI, QU_PF, CP, EX, IS, SY, BK, CR, LF, ZW,
                    ]
                    .contains(&nncls)
                    {
                        ncls = QU_PF;
                    }
                }
            }
            if (lcls >= SP || lcls == ZW || lcls == GL || lcls == CB) && (ncls == HY || ncls == HH)
            {
                // LB20a
                ncls = if ncls == HH { WS_BA } else { WS_HY };
            }
            if cls == AP && c == DOTTED_CIRCLE && lcls != SP {
                // LB28a
                break 'body To::Next;
            }
            loop {
                let old = brs;
                brs = brahmic(brs, ncls, c);
                if brs == old {
                    break;
                }
                match brs {
                    Brahmic::Start => brs_start = i,
                    Brahmic::VI2 => {
                        at[(brs_start + 1).min(i)..i].fill(false);
                        brs_start = i;
                        break 'body To::Next;
                    }
                    Brahmic::Restart => {
                        brs = Brahmic::Start;
                        brs_start = i - 1;
                        continue;
                    }
                    Brahmic::VF2 | Brahmic::FourVF | Brahmic::VIAK3 => {
                        at[(brs_start + 1).min(i)..i].fill(false);
                        if brs == Brahmic::VIAK3 {
                            brs = Brahmic::Start;
                            brs_start = i;
                        } else {
                            brs = Brahmic::None;
                        }
                        break 'body To::Next;
                    }
                    Brahmic::Four | Brahmic::None => {}
                }
                break;
            }
            if ncls == IS && lcls == SP && next_class == Some(NU) {
                // LB15c
                at[i] = true;
                break 'body To::Next;
            }
            if lcls == HL && (ncls == HY || ncls == HH) {
                // LB21a
                ncls = HYBA;
                break 'body To::Next;
            }
            // LB25
            {
                let necur = number::class(ncls);
                let mut neact = number::ACTION[nelast][necur];
                if neactlast == number::C_NEED_NU && necur != number::NU {
                    neact = number::NONE;
                } else if neactlast == number::NEED_OP_NU {
                    neact = match necur {
                        number::OP => number::C_NEED_IS_NU,
                        number::NU => number::CONTINUE,
                        _ => number::NONE,
                    };
                } else if neactlast == number::C_NEED_IS_NU {
                    neact = match necur {
                        number::IS => number::C_NEED_NU,
                        number::NU => number::CONTINUE,
                        _ => number::NONE,
                    };
                }
                match neact {
                    number::BREAK => {
                        at[(nestart + 1).min(i)..i].fill(false);
                        nelast = number::XX;
                    }
                    number::NONE => nelast = number::XX,
                    number::NEED_OP_NU | number::START => {
                        if neactlast == number::START || neactlast == number::CONTINUE {
                            at[(nestart + 1).min(i)..i].fill(false);
                        }
                        nestart = i;
                        nelast = necur;
                    }
                    _ => nelast = necur,
                }
                neactlast = neact;
            }
            if ncls == QU && lcls != SP && lcls != ZW {
                // LB19a
                let next_not_east_asian = chars.get(i + 1).is_none_or(|&(_, n)| {
                    let nncls = class(n);
                    nncls != CM && nncls <= SP && !props(n).east_asian()
                });
                if !last.east_asian() || next_not_east_asian {
                    ncls = QU_19;
                }
            }
            if lcls >= CR {
                // LB4, LB5
                if lcls > CR || ncls != LF {
                    at[i] = true;
                }
                break 'body To::Next;
            }
            if ncls >= SP {
                // LB6, LB7
                break 'body if ncls > SP { To::Next } else { To::NoClsUpdate };
            }
            // LB19
            if ((ncls == QU || ncls == QU_19) && !prop.initial_quote()
                || cls == QU && !last.final_quote())
                && lcls != SP
                && lcls != ZW
                && ncls != CM
            {
                break 'body To::Next;
            }
            if (ncls == CM || ncls == ZWJ) && lcls != ZW && lcls < SP {
                // LB9
                break 'body To::NoClsUpdate;
            }
            if lcls == ZWJ {
                // LB8a
                break 'body To::Next;
            }
            if ncls == RI && lcls == RI {
                // LB30a
                ncls = SP;
                break 'body To::Next;
            }
            if ncls == EM && last.unassigned_pictographic() && lcls != SP {
                // LB30b
                break 'body To::Next;
            }
            if cls >= SA {
                cls = AL;
            }
            let mut tcls = cls;
            // LB10
            if tcls == CM || tcls == ZWJ {
                tcls = AL;
                last = Props::LETTER;
            }
            if ncls == CM || ncls == ZWJ {
                ncls = AL;
                prop = Props::LETTER;
            }
            let column = if ncls < ZWJ { ncls } else { AL };
            match TABLE[tcls as usize][column as usize] {
                DB => at[i] = true,
                IB => {
                    if lcls == SP {
                        at[i] = true;
                    }
                }
                CI => {
                    if lcls != SP {
                        break 'body To::NoClsUpdate;
                    }
                    at[i] = true;
                }
                CPB => {
                    if lcls != SP {
                        break 'body To::NoClsUpdate;
                    }
                }
                HHB => {
                    if lcls != HL {
                        at[i] = true;
                    }
                }
                INB => {
                    if prop.east_asian() || lcls == SP {
                        at[i] = true;
                    }
                }
                DN if neactlast == number::NONE || neactlast > number::BREAK => {
                    at[i] = true;
                }
                _ => {}
            }
            To::Next
        };
        if to == To::Next && ncls != CM && ncls != ZWJ {
            cls = ncls;
            last = prop;
        }
        lcls = ncls;
    }
    if number::ACTION[nelast][number::XX] == number::BREAK {
        // LB25
        at[(nestart + 1).min(len)..len].fill(false);
    }

    let mut out = vec![false; text.len() + 1];
    for (k, &(offset, _)) in chars.iter().enumerate().skip(1) {
        out[offset] = at[k];
    }
    out[text.len()] = true;
    out
}

/// `Tailored::getCharAttributes` over the opportunities `at` of `text` and over `characters`,
/// the boundaries of what Qt takes for one character (QCharAttributes::graphemeBoundary):
/// Thai is broken where libthai's dictionary finds words, when the library is there, and
/// Myanmar before every syllable; a syllable of an Indic script, of Tibetan and of Khmer is
/// one character, whatever the grapheme clusters of Unicode are. All go over one item of the
/// text layout at a time (QTextEngine::attributes hands its items over as the stretches of
/// one script); `item` is the ISO 15924 tag of the script of the item that starts at a byte.
pub fn tailor(
    text: &str,
    item: &dyn Fn(usize) -> Option<[u8; 4]>,
    at: &mut [bool],
    characters: &mut [bool],
) {
    let mut stretch = |from: usize, to: usize, script: Option<[u8; 4]>| {
        let (text, characters) = (&text[from..to], &mut characters[from..to]);
        match script.as_ref() {
            Some(b"Thai") => thai(text, &mut at[from..to]),
            Some(b"Mymr") => myanmar(text, &mut at[from..to]),
            Some(b"Tibt") => syllables(text, characters, &tibetan_syllable_end),
            Some(b"Khmr") => syllables(text, characters, &khmer_syllable_end),
            Some(
                script @ (b"Deva" | b"Beng" | b"Guru" | b"Gujr" | b"Orya" | b"Taml" | b"Telu"
                | b"Knda" | b"Mlym" | b"Sinh"),
            ) => syllables(text, characters, &|units, start| {
                indic_syllable_end(script, units, start)
            }),
            _ => {}
        }
    };
    let (mut start, mut script) = (0, item(0));
    for (pos, _) in text.char_indices().skip(1) {
        if let Some(next) = item(pos) {
            stretch(start, pos, script);
            (start, script) = (pos, Some(next));
        }
    }
    stretch(start, text.len(), script);
}

/// A UTF-16 unit of a stretch and the byte its character starts at, None in a low surrogate.
type Unit = (u16, Option<usize>);

/// `indicAttributes`, `tibetanAttributes` and `khmerAttributes`: a character starts where a
/// syllable of the stretch `text` does, and nowhere inside one. `end` is the end of the
/// syllable that starts at a unit.
fn syllables(text: &str, characters: &mut [bool], end: &dyn Fn(&[Unit], usize) -> usize) {
    let units = units(text);
    let mut i = 0;
    while i < units.len() {
        let boundary = end(&units, i).max(i + 1);
        for (k, unit) in units.iter().enumerate().take(boundary).skip(i) {
            if let Some(pos) = unit.1 {
                characters[pos] = k == i;
            }
        }
        i = boundary;
    }
}

/// `form`: what a UTF-16 unit is in a syllable of an Indic script, by the numbers of
/// `INDIC_FORMS`.
fn indic_form(unit: u16) -> u8 {
    let digit = |d: u8| if d >= b'a' { d - b'a' + 10 } else { d - b'0' };
    match unit {
        0x900..=0xdff => digit(INDIC_FORMS[unit as usize - 0x900]),
        0x25cc => indic::CONSONANT,
        0x200c | 0x200d => indic::CONTROL,
        _ => indic::OTHER,
    }
}

/// The numbers of `INDIC_FORMS`.
mod indic {
    pub const CONSONANT: u8 = 1;
    pub const NUKTA: u8 = 2;
    pub const HALANT: u8 = 3;
    pub const MATRA: u8 = 4;
    pub const VOWEL_MARK: u8 = 5;
    pub const STRESS_MARK: u8 = 6;
    pub const INDEPENDENT_VOWEL: u8 = 7;
    pub const LENGTH_MARK: u8 = 8;
    pub const CONTROL: u8 = 9;
    pub const OTHER: u8 = 10;
}

/// `indic_nextSyllableBoundary`: the end of the syllable that starts at unit `start` of a
/// stretch of `script`.
fn indic_syllable_end(script: &[u8; 4], units: &[Unit], start: usize) -> usize {
    use indic::*;
    const ZWJ: u16 = 0x200d;
    let uc = |pos: usize| units[start + pos].0;
    let len = units.len() - start;
    let mut state = indic_form(uc(0));
    let mut pos = 1;
    if state != CONSONANT && state != INDEPENDENT_VOWEL {
        return start + pos;
    }
    while pos < len {
        let (unit, before) = (uc(pos), uc(pos - 1));
        let mut form = indic_form(unit);
        let goes_on = match form {
            CONTROL => {
                form = state;
                // Bengali and Kannada write ya-phala after ra with a ZWJ between them; any
                // other control character is the last of its syllable.
                let joins = state == HALANT && unit == ZWJ
                    || state == CONSONANT && script == b"Beng" && before == 0x9b0 && unit == ZWJ
                    || state == CONSONANT && script == b"Knda" && before == 0xcb0 && unit == ZWJ;
                if !joins {
                    return start + pos + 1;
                }
                true
            }
            CONSONANT => state == HALANT && (script != b"Sinh" || before == ZWJ),
            HALANT => {
                // Sinhala has the halant in some of its vowel signs, and Malayalam after
                // the vowel sign u: the syllable goes on as after the vowel sign.
                if state == MATRA && (script == b"Sinh" || script == b"Mlym" && before == 0xd41) {
                    pos += 1;
                    continue;
                }
                state == NUKTA
                    || state == CONSONANT
                    || script == b"Beng" && pos == 1 && matches!(uc(0), 0x985 | 0x98f)
            }
            NUKTA => state == CONSONANT,
            STRESS_MARK | VOWEL_MARK | MATRA => {
                form == STRESS_MARK && state == VOWEL_MARK
                    || form != MATRA && matches!(state, MATRA | LENGTH_MARK | INDEPENDENT_VOWEL)
                    || matches!(state, CONSONANT | NUKTA | MATRA)
                    || script == b"Beng" && unit == 0x9be && before == 0x985
            }
            LENGTH_MARK => state == MATRA,
            _ => false,
        };
        if !goes_on {
            break;
        }
        state = form;
        pos += 1;
    }
    start + pos
}

/// `tibetan_nextSyllableBoundary`: the end of the syllable that starts at unit `start`.
fn tibetan_syllable_end(units: &[Unit], start: usize) -> usize {
    const HEAD: u8 = 1;
    const SUBJOINED_CONSONANT: u8 = 2;
    const SUBJOINED_VOWEL: u8 = 3;
    const VOWEL: u8 = 4;
    let form = |unit: u16| match unit {
        0xf40..=0xfbf => TIBETAN_FORMS[unit as usize - 0xf40] - b'0',
        _ => 0,
    };
    let mut state = form(units[start].0);
    let mut pos = start + 1;
    if state != HEAD {
        return pos;
    }
    while pos < units.len() {
        match form(units[pos].0) {
            next @ (SUBJOINED_CONSONANT | SUBJOINED_VOWEL)
                if matches!(state, HEAD | SUBJOINED_CONSONANT) =>
            {
                state = next
            }
            VOWEL if matches!(state, HEAD | SUBJOINED_CONSONANT | SUBJOINED_VOWEL) => {}
            _ => break,
        }
        pos += 1;
    }
    pos
}

/// `khmer_nextSyllableBoundary`: the end of the syllable that starts at unit `start`.
fn khmer_syllable_end(units: &[Unit], start: usize) -> usize {
    let class = |unit: u16| match unit {
        0x200d => 11,
        0x200c => 4,
        0x1780..=0x17df => match KHMER_CLASSES[unit as usize - 0x1780] {
            b'a' => 10,
            digit => (digit - b'0') as usize,
        },
        _ => 0,
    };
    let mut state = 0usize;
    let mut pos = start;
    while pos < units.len() {
        match KHMER_STATES[state][class(units[pos].0)] {
            -1 => break,
            next => state = next as usize,
        }
        pos += 1;
    }
    pos
}

/// `indicForms`: what a character of U+0900..U+0DFF is in a syllable, a hexadecimal digit a
/// character: 0 invalid, 1 consonant, 2 nukta, 3 halant, 4 matra, 5 vowel mark, 6 stress mark,
/// 7 independent vowel, 8 length mark, 9 control, a other.
const INDIC_FORMS: &[u8; 0x500] = b"\
    0555777777777777777771111111111111111111111111111111111111002a44\
    4444444444444300a6666000111111117755aaaaaaaaaaaaaaaaaaaaaaa11111\
    0555077777777007700771111111111111111111101111111010001111002a44\
    444440044004431000000005000011017755aaaaaaaaaaaa11aaaaaaaaaaaaaa\
    0555077777700007700771111111111111111111101111111011011011002a44\
    44400004400443000000000001111010aa00aaaaaaaaaaaa6611aaaaaaaaaaaa\
    0555077777777707770771111111111111111111101111111011011111002a44\
    4444440444044300a0000000000000007755aaaaaaaaaaaaaaaaaaaaaaaaaaaa\
    0555077777777007700771111111111111111111101111111011011111002a44\
    4444000440044300a000008800001101770000aaaaaaaaaaa1aaaaaaaaaaaaaa\
    005a077777700077707771000110101100011000111000111111111111000044\
    44400044404443000000000800000000000000aaaaaaaaaaaaaaaaaaaaaaaaaa\
    0555077777777077707771111111111111111111101111111111011111000044\
    44444044404443000000084000000000770000aaaaaaaaaaaaaaaaaaaaaaaaaa\
    0055077777777077707771111111111111111111101111111111011111002a44\
    44444044404443000000088000000010775500aaaaaaaaaaaaaaaaaaaaaaaaaa\
    0055077777777077707771111111111111111111101111111111111111000044\
    44440044404443000000000400000000770000aaaaaaaaaaaaaaaaaaaaaaaaaa\
    0055077777777777777777700011111111111111111111111101111111110100\
    1111111000300004444440404444444400000000000000000044aaaaaaaaaaaa\
";

/// `tibetanForm`, from U+0F40: 0 other, 1 head consonant, 2 subjoined consonant, 3 subjoined
/// vowel, 4 vowel.
const TIBETAN_FORMS: &[u8; 0x80] = b"\
    1111111111111111111111111111111111111111111100000444444444444444\
    4444444400000000222222222222222222222222222222222222222222222000\
";

/// `khmerCharClasses`, from U+1780, a column of `KHMER_STATES`: 0 other, 1 to 3 consonant of
/// type 1 (and independent vowel), 2 and 3, 5 consonant shifter, 6 robat, 7 coeng, 8 dependent
/// vowel, 9 sign above, a sign after. ZWNJ is 4 and ZWJ 11.
const KHMER_CLASSES: &[u8; 0x60] = b"\
    1113111131111311111131111321113313111111111111111111888888888888\
    8888889aa55969999979000000000900\
";

/// `khmerStateTable`: the state after a character of a class, -1 where the syllable ends.
#[rustfmt::skip]
const KHMER_STATES: [[i8; 12]; 21] = [
    [ 1,  2,  2,  2,  1,  1,  1,  6,  1,  1,  1,  2],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [-1, -1, -1, -1,  3,  4,  5,  6, 16, 17,  1, -1],
    [-1, -1, -1, -1, -1,  4, -1, -1, 16, -1, -1, -1],
    [-1, -1, -1, -1, 15, -1, -1,  6, 16, 17,  1, 14],
    [-1, -1, -1, -1, -1, -1, -1, -1, 20, -1,  1, -1],
    [-1,  7,  8,  9, -1, -1, -1, -1, -1, -1, -1, -1],
    [-1, -1, -1, -1, 12, 13, -1, 10, 16, 17,  1, 14],
    [-1, -1, -1, -1, 12, 13, -1, -1, 16, 17,  1, 14],
    [-1, -1, -1, -1, 12, 13, -1, 10, 16, 17,  1, 14],
    [-1, 11, 11, 11, -1, -1, -1, -1, -1, -1, -1, -1],
    [-1, -1, -1, -1, 15, -1, -1, -1, 16, 17,  1, 14],
    [-1, -1, -1, -1, -1, 13, -1, -1, 16, -1, -1, -1],
    [-1, -1, -1, -1, 15, -1, -1, -1, 16, 17,  1, 14],
    [-1, -1, -1, -1, -1, -1, -1, -1, 16, -1, -1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, 16, -1, -1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, 17,  1, 18],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1,  1, 18],
    [-1, -1, -1, -1, -1, -1, -1, 19, -1, -1, -1, -1],
    [-1,  1, -1,  1, -1, -1, -1, -1, -1, -1, -1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1,  1, -1],
];

/// The UTF-16 code units of `text`, each with its byte offset when a character starts there.
fn units(text: &str) -> Vec<Unit> {
    let mut units = Vec::with_capacity(text.len());
    for (pos, c) in text.char_indices() {
        let mut pair = [0; 2];
        for (k, &unit) in c.encode_utf16(&mut pair).iter().enumerate() {
            units.push((unit, (k == 0).then_some(pos)));
        }
    }
    units
}

type ThBrk = *mut c_void;

/// libthai's word breaker.
struct LibThai {
    _lib: libloading::Library,
    state: ThBrk,
    find_breaks: unsafe extern "C" fn(ThBrk, *const c_uchar, *mut c_int, usize) -> c_int,
}

// SAFETY: `state` is only read once th_brk_new has made it (libthai documents
// th_brk_find_breaks as safe to call from several threads on one ThBrk), and it is never
// freed.
unsafe impl Send for LibThai {}
// SAFETY: as for Send.
unsafe impl Sync for LibThai {}

/// The library Qt opens (`QLibrary("thai", 0)`), or `None` when it is not installed: Thai is
/// then broken at spaces only.
fn lib_thai() -> Option<&'static LibThai> {
    static LIB: OnceLock<Option<LibThai>> = OnceLock::new();
    LIB.get_or_init(|| {
        // SAFETY: loading libthai runs no initialisers with preconditions.
        let lib = unsafe { libloading::Library::new("libthai.so.0") }.ok()?;
        // SAFETY: `ThBrk *th_brk_new(const char *dictpath)` (thai/thbrk.h, libthai 0.1.25 on).
        let new: unsafe extern "C" fn(*const u8) -> ThBrk =
            *unsafe { lib.get(c"th_brk_new") }.ok()?;
        // SAFETY: `int th_brk_find_breaks(ThBrk *, const thchar_t *, int pos[], size_t)`
        // (thai/thbrk.h); the library lives in the static next to the copied pointer.
        let find_breaks = *unsafe { lib.get(c"th_brk_find_breaks") }.ok()?;
        // SAFETY: a null path asks for the default dictionary.
        let state = unsafe { new(std::ptr::null()) };
        (!state.is_null()).then_some(LibThai {
            _lib: lib,
            state,
            find_breaks,
        })
    })
    .as_ref()
}

/// libthai is installed: Thai is broken between words. `impulse-shell --version` says so, and
/// tools/install.sh tells the user when it is not.
pub fn thai_library() -> bool {
    lib_thai().is_some()
}

/// `thaiAssignAttributes`: the opportunities of a Thai stretch are those libthai finds, also
/// at its start and around what is not Thai in it.
fn thai(text: &str, at: &mut [bool]) {
    let Some(lib) = lib_thai() else {
        return;
    };
    let units = units(text);
    // `to_tis620`
    let mut tis: Vec<u8> = units
        .iter()
        .map(|&(unit, _)| match unit {
            0..=0xa0 => unit as u8,
            0xe01..=0xe5b => (unit - 0xe00 + 0xa0) as u8,
            _ => 0xff,
        })
        .collect();
    tis.push(0);
    let mut found: Vec<c_int> = vec![0; units.len()];
    // SAFETY: `tis` ends in a NUL as th_brk_find_breaks asks, and `found` has room for the
    // `found.len()` positions it may write.
    let count =
        unsafe { (lib.find_breaks)(lib.state, tis.as_ptr(), found.as_mut_ptr(), found.len()) };
    at.fill(false);
    for &unit in found.iter().take(usize::try_from(count).unwrap_or(0)) {
        let pos = usize::try_from(unit)
            .ok()
            .and_then(|unit| units.get(unit)?.1);
        if let Some(pos) = pos {
            at[pos] = true;
        }
    }
}

/// `myanmarAttributes`: a line may be broken before every syllable of a Myanmar stretch.
fn myanmar(text: &str, at: &mut [bool]) {
    let units = units(text);
    let len = units.len();
    let mut i = 0;
    while i < len {
        if let Some(pos) = units[i].1 {
            at[pos] = true;
        }
        let mut boundary = myanmar_syllable_end(&units, i);
        if boundary + 1 > len {
            boundary = len;
        }
        i = boundary.max(i + 1);
    }
}

/// `myanmar_nextSyllableBoundary`: the end of the syllable that starts at unit `start`.
fn myanmar_syllable_end(units: &[Unit], start: usize) -> usize {
    let mut state = 0usize;
    let mut pos = start;
    while pos < units.len() {
        match MYANMAR_STATES[state][myanmar_class(units[pos].0)] {
            -1 => break,
            // A medial or a virama out of order starts the next syllable.
            -2 => return pos - 1,
            next => state = next as usize,
        }
        pos += 1;
    }
    pos
}

/// `getMyanmarCharClass`, the column of `MYANMAR_STATES`: other (0), consonant with and
/// without a subscript form, nga, ya, ra, wa, ha, independent vowel, ZWNJ, virama, the
/// dependent vowels e, below, above and aa, the signs above, below and after, ZWJ (18).
fn myanmar_class(unit: u16) -> usize {
    match unit {
        0x200d => 18,
        0x200c => 9,
        0x1004 => 3,
        0x100a | 0x1020 | 0x1021 => 2,
        0x101a => 4,
        0x101b => 5,
        0x101d => 6,
        0x101f => 7,
        0x1000..=0x101e => 1,
        0x1023..=0x1027 | 0x1029 | 0x102a => 8,
        0x102c => 14,
        0x102d | 0x102e | 0x1032 => 13,
        0x102f | 0x1030 => 12,
        0x1031 => 11,
        0x1036 => 15,
        0x1037 => 16,
        0x1038 => 17,
        0x1039 => 10,
        _ => 0,
    }
}

/// `mymrStateTable`: the next state by state and class; -1 ends the syllable before the
/// character, -2 one character earlier.
#[rustfmt::skip]
static MYANMAR_STATES: [[i8; 19]; 28] = [
    [ 1,  4,  4,  2,  4,  4,  4,  4, 24,  1, 27, 17, 18, 19, 20, 21,  1,  1,  4],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1,  3, 17, 18, 19, 20, 21, -1, -1,  4],
    [-1,  4,  4,  4,  4,  4,  4,  4, -1, 23, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1,  5, 17, 18, 19, 20, 21,  1,  1, -1],
    [-2,  6, -2, -2,  7,  8,  9, 10, -2, 23, -2, -2, -2, -2, -2, -2, -2, -2, -2],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 25, 17, 18, 19, 20, 21, -1, -1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 12, 17, 18, 19, 20, 21, -1, -1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 12, 17, 18, 19, 20, 21, -1, -1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 12, 17, 18, 19, 20, 21, -1, -1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 17, 18, 19, 20, 21, -1, -1, -1],
    [-1, -1, -1, -1,  7,  8,  9, 10, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [-2, -2, -2, -2, -2, -2, 13, 14, -2, -2, -2, -2, -2, -2, -2, -2, -2, -2, -2],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 15, 17, 18, 19, 20, 21, -1, -1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 17, 18, 19, 20, 21, -1, -1, -1],
    [-2, -2, -2, -2, -2, -2, -2, 16, -2, -2, -2, -2, -2, -2, -2, -2, -2, -2, -2],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 17, 18, 19, 20, 21, -1, -1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 20, 21,  1,  1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 19, -1, 21,  1,  1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,  1,  1,  1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 22, -1, -1, -1, -1, -1,  1,  1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,  1,  1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, 23, -1, -1, -1, -1, -1, -1, -1, -1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,  1,  1, -1],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,  1, -1],
    [-2, -2, -2, -2, 26, 26, -2, -2, -2, -2, -2, -2, -2, -2, -2, -2, -2, -2, -2],
    [-1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 12, 17, 18, 19, 20, 21, -1,  1, -1],
    [-1,  6, -1, -1,  7,  8,  9, 10, -1, 23, -1, -1, -1, -1, -1, -1, -1, -1, -1],
];
