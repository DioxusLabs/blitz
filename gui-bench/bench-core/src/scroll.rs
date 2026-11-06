//! The text scroll workload: a long document with mixed fonts and sizes, scrolled at a
//! fixed rate of [`SCROLL_STEP`] logical px per frame.

pub const SCROLL_STEP: f32 = 20.0;
pub const SECTIONS: usize = 200;

/// Block styles. Each toolkit maps these to its own text styling:
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    /// 24px bold sans-serif.
    Heading,
    /// 16px regular sans-serif.
    Paragraph,
    /// 16px italic sans-serif.
    Emphasis,
    /// 14px monospace.
    Code,
}

impl BlockKind {
    pub fn font_size(self) -> f32 {
        match self {
            BlockKind::Heading => 24.0,
            BlockKind::Paragraph | BlockKind::Emphasis => 16.0,
            BlockKind::Code => 14.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Block {
    pub kind: BlockKind,
    pub text: String,
}

const WORDS: &[&str] = &[
    "lorem",
    "ipsum",
    "dolor",
    "sit",
    "amet",
    "consectetur",
    "adipiscing",
    "elit",
    "sed",
    "do",
    "eiusmod",
    "tempor",
    "incididunt",
    "ut",
    "labore",
    "et",
    "dolore",
    "magna",
    "aliqua",
    "enim",
    "ad",
    "minim",
    "veniam",
    "quis",
    "nostrud",
    "exercitation",
    "ullamco",
    "laboris",
    "nisi",
    "aliquip",
    "ex",
    "ea",
    "commodo",
    "consequat",
    "duis",
    "aute",
    "irure",
    "in",
    "reprehenderit",
    "voluptate",
    "velit",
    "esse",
    "cillum",
    "fugiat",
    "nulla",
    "pariatur",
    "excepteur",
    "sint",
    "occaecat",
    "cupidatat",
    "non",
    "proident",
    "sunt",
    "culpa",
    "qui",
    "officia",
    "deserunt",
    "mollit",
    "anim",
    "id",
    "est",
    "laborum",
];

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 33) as usize
    }
    fn words(&mut self, n: usize) -> String {
        let mut s = String::new();
        for i in 0..n {
            if i > 0 {
                s.push(' ');
            }
            s.push_str(WORDS[self.next() % WORDS.len()]);
        }
        s
    }
}

/// The document: [`SECTIONS`] sections of a heading, 2-4 paragraphs (some italic) and
/// a line of code. Deterministic, so every toolkit lays out the same text.
pub fn document() -> Vec<Block> {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let mut blocks = Vec::new();
    for section in 0..SECTIONS {
        let n = 3 + rng.next() % 4;
        let title = rng.words(n);
        blocks.push(Block {
            kind: BlockKind::Heading,
            text: format!("{}. {title}", section + 1),
        });
        for p in 0..2 + rng.next() % 3 {
            let kind = if p == 1 && section % 3 == 0 {
                BlockKind::Emphasis
            } else {
                BlockKind::Paragraph
            };
            let n = 40 + rng.next() % 80;
            blocks.push(Block {
                kind,
                text: rng.words(n),
            });
        }
        let mut word = || WORDS[rng.next() % WORDS.len()];
        let (a, b, c) = (word(), word(), word());
        let code = format!("let {a} = {b}({}, {c});", rng.next() % 1000);
        blocks.push(Block {
            kind: BlockKind::Code,
            text: code,
        });
    }
    blocks
}
