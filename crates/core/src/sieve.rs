//! Xenakis sieves: residue-class formulas over the 128 MIDI keys.
//!
//! A sieve is built from residue atoms `M@R` (every key congruent to `R`
//! modulo `M`) combined with union, intersection, and complement, the
//! formalism Xenakis used to carve pitch and rhythm lattices out of the
//! chromatic continuum. The whole expression is evaluated once at parse
//! time into a 128-bit membership set, so every query afterwards is a bit
//! test with no allocation.

/// A set of MIDI keys defined by a residue-class expression.
///
/// Grammar, loosest to tightest binding (whitespace allowed anywhere
/// between tokens):
///
/// ```text
/// expr   := term ("|" term)*        union
/// term   := factor ("&" factor)*    intersection
/// factor := "!" factor              complement (within keys 0..=127)
///         | "(" expr ")"
///         | atom
/// atom   := M "@" R                 keys where key % M == R
/// ```
///
/// So `!` binds tightest, `&` binds over `|`: `!3@0 & 4@1 | 12@7` reads as
/// `((!3@0) & 4@1) | 12@7`. The modulus `M` must be 1..=127 and the
/// residue `R` must be less than `M`. An expression that matches no key at
/// all is rejected at parse time, so a parsed sieve is never empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sieve {
    /// Bit `k` set means key `k` is a member.
    members: u128,
}

impl Sieve {
    /// Parse a sieve expression. Errors are human-readable and carry the
    /// 1-based position of the offending token.
    pub fn parse(expr: &str) -> Result<Sieve, String> {
        let mut p = Parser {
            bytes: expr.as_bytes(),
            pos: 0,
        };
        if p.peek().is_none() {
            return Err("empty sieve expression".to_string());
        }
        let members = p.expr()?;
        if let Some(c) = p.peek() {
            return Err(p.err(&format!("unexpected '{}'", c as char)));
        }
        if members == 0 {
            return Err("sieve matches no keys".to_string());
        }
        Ok(Sieve { members })
    }

    /// Whether `key` is a member. Keys past 127 never are.
    pub fn contains(&self, key: u8) -> bool {
        key < 128 && self.members >> key & 1 == 1
    }

    /// The member closest to `key`, ties breaking downward. `None` never
    /// happens for a parsed sieve (empty sieves are rejected), but the
    /// signature stays honest.
    pub fn nearest(&self, key: u8) -> Option<u8> {
        let key = key.min(127);
        for d in 0..=127u8 {
            if key >= d && self.contains(key - d) {
                return Some(key - d);
            }
            let above = key as u16 + d as u16;
            if above <= 127 && self.contains(above as u8) {
                return Some(above as u8);
            }
        }
        None
    }

    /// The member at or above `key`, or `None` when the sieve has no
    /// member that high.
    pub fn up(&self, key: u8) -> Option<u8> {
        (key..=127).find(|&k| self.contains(k))
    }

    /// The member at or below `key`, or `None` when the sieve has no
    /// member that low.
    pub fn down(&self, key: u8) -> Option<u8> {
        (0..=key.min(127)).rev().find(|&k| self.contains(k))
    }
}

/// Recursive-descent evaluator: each production returns the 128-bit set it
/// denotes, so no AST is ever built.
struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn peek(&mut self) -> Option<u8> {
        while matches!(self.bytes.get(self.pos), Some(b) if b.is_ascii_whitespace()) {
            self.pos += 1;
        }
        self.bytes.get(self.pos).copied()
    }

    fn err(&self, msg: &str) -> String {
        format!("{msg} at position {}", self.pos + 1)
    }

    fn expr(&mut self) -> Result<u128, String> {
        let mut set = self.term()?;
        while self.peek() == Some(b'|') {
            self.pos += 1;
            set |= self.term()?;
        }
        Ok(set)
    }

    fn term(&mut self) -> Result<u128, String> {
        let mut set = self.factor()?;
        while self.peek() == Some(b'&') {
            self.pos += 1;
            set &= self.factor()?;
        }
        Ok(set)
    }

    fn factor(&mut self) -> Result<u128, String> {
        match self.peek() {
            // The complement of a u128 is exactly the complement within
            // the 128-key universe.
            Some(b'!') => {
                self.pos += 1;
                Ok(!self.factor()?)
            }
            Some(b'(') => {
                let open = self.pos;
                self.pos += 1;
                let set = self.expr()?;
                if self.peek() != Some(b')') {
                    return Err(format!("unclosed '(' at position {}", open + 1));
                }
                self.pos += 1;
                Ok(set)
            }
            Some(c) if c.is_ascii_digit() => self.atom(),
            Some(c) => Err(self.err(&format!(
                "expected a residue atom, '!', or '(', found '{}'",
                c as char
            ))),
            None => Err(self.err("expected a residue atom, '!', or '('")),
        }
    }

    fn atom(&mut self) -> Result<u128, String> {
        let (modulus, at) = self.number("modulus")?;
        if !(1..=127).contains(&modulus) {
            return Err(format!(
                "modulus must be 1..=127, got {modulus} at position {}",
                at + 1
            ));
        }
        if self.peek() != Some(b'@') {
            return Err(self.err("expected '@' after the modulus"));
        }
        self.pos += 1;
        let (residue, at) = self.number("residue")?;
        if residue >= modulus {
            return Err(format!(
                "residue {residue} must be less than modulus {modulus} at position {}",
                at + 1
            ));
        }
        let mut set = 0u128;
        let mut key = residue;
        while key < 128 {
            set |= 1 << key;
            key += modulus;
        }
        Ok(set)
    }

    /// A decimal number, returned with the position where it started.
    fn number(&mut self, what: &str) -> Result<(u16, usize), String> {
        self.peek();
        let start = self.pos;
        while matches!(self.bytes.get(self.pos), Some(b) if b.is_ascii_digit()) {
            self.pos += 1;
        }
        let digits = &self.bytes[start..self.pos];
        if digits.is_empty() {
            return Err(format!("expected a {what} at position {}", start + 1));
        }
        if digits.len() > 3 {
            return Err(format!("{what} out of range at position {}", start + 1));
        }
        let value = digits.iter().fold(0u16, |v, b| v * 10 + (b - b'0') as u16);
        Ok((value, start))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn members(expr: &str) -> Vec<u8> {
        let sieve = Sieve::parse(expr).unwrap();
        (0..=127).filter(|&k| sieve.contains(k)).collect()
    }

    #[test]
    fn atom_selects_a_residue_class() {
        assert_eq!(members("12@0"), (0..=127).step_by(12).collect::<Vec<_>>());
        assert_eq!(members("12@7"), (7..=127).step_by(12).collect::<Vec<_>>());
        assert_eq!(members("1@0"), (0..=127).collect::<Vec<_>>());
    }

    #[test]
    fn union_and_intersection() {
        assert_eq!(members("12@0 | 12@7"), {
            let mut all: Vec<u8> = (0..=127).filter(|k| k % 12 == 0 || k % 12 == 7).collect();
            all.sort_unstable();
            all
        });
        // Keys divisible by both 3 and 4: divisible by 12.
        assert_eq!(members("3@0 & 4@0"), members("12@0"));
    }

    #[test]
    fn complement_within_the_keyboard() {
        assert_eq!(
            members("!2@0"),
            (0..=127).filter(|k| k % 2 == 1).collect::<Vec<_>>()
        );
    }

    #[test]
    fn intersection_binds_over_union() {
        // 2@0 | 3@0 & 4@1 must read as 2@0 | (3@0 & 4@1): 9 is in 3@0 and
        // 4@1, 2 is even, 3 is in neither.
        let sieve = Sieve::parse("2@0 | 3@0 & 4@1").unwrap();
        assert!(sieve.contains(9));
        assert!(sieve.contains(2));
        assert!(!sieve.contains(3));
        // (2@0 | 3@0) & 4@1 excludes 2 (even but not 4@1) and keeps 9.
        let grouped = Sieve::parse("(2@0 | 3@0) & 4@1").unwrap();
        assert!(!grouped.contains(2));
        assert!(grouped.contains(9));
    }

    #[test]
    fn complement_binds_tightest() {
        // !2@0 & 3@0 must read as (!2@0) & 3@0: odd multiples of three.
        assert_eq!(
            members("!2@0 & 3@0"),
            (0..=127)
                .filter(|k| k % 2 == 1 && k % 3 == 0)
                .collect::<Vec<_>>()
        );
        // !(2@0 & 3@0) instead removes only the multiples of six.
        assert_eq!(
            members("!(2@0 & 3@0)"),
            (0..=127).filter(|k| k % 6 != 0).collect::<Vec<_>>()
        );
    }

    #[test]
    fn double_complement_is_identity() {
        assert_eq!(members("!!12@0"), members("12@0"));
    }

    #[test]
    fn whitespace_is_free() {
        assert_eq!(members("  12 @ 0 |\t12@7  "), members("12@0|12@7"));
    }

    #[test]
    fn bad_modulus_is_rejected() {
        assert!(Sieve::parse("0@0").unwrap_err().contains("modulus"));
        assert!(Sieve::parse("128@0").unwrap_err().contains("modulus"));
        assert!(Sieve::parse("9999@0").unwrap_err().contains("out of range"));
    }

    #[test]
    fn residue_must_be_below_the_modulus() {
        let err = Sieve::parse("12@12").unwrap_err();
        assert!(err.contains("residue 12"), "{err}");
        assert!(Sieve::parse("12@13").is_err());
    }

    #[test]
    fn empty_and_missing_pieces_are_rejected() {
        assert_eq!(Sieve::parse("").unwrap_err(), "empty sieve expression");
        assert_eq!(Sieve::parse("   ").unwrap_err(), "empty sieve expression");
        assert!(Sieve::parse("12").unwrap_err().contains("'@'"));
        assert!(Sieve::parse("12@").unwrap_err().contains("residue"));
        assert!(Sieve::parse("@3").is_err());
        assert!(Sieve::parse("12@0 |").is_err());
    }

    #[test]
    fn unbalanced_parens_are_rejected() {
        let err = Sieve::parse("(12@0 | 12@7").unwrap_err();
        assert!(err.contains("unclosed '(' at position 1"), "{err}");
        let err = Sieve::parse("12@0)").unwrap_err();
        assert!(err.contains("unexpected ')' at position 5"), "{err}");
    }

    #[test]
    fn errors_carry_a_position() {
        let err = Sieve::parse("12@0 | x").unwrap_err();
        assert!(err.contains("position 8"), "{err}");
    }

    #[test]
    fn empty_result_is_rejected() {
        assert_eq!(
            Sieve::parse("2@0 & 2@1").unwrap_err(),
            "sieve matches no keys"
        );
        assert_eq!(Sieve::parse("!1@0").unwrap_err(), "sieve matches no keys");
    }

    #[test]
    fn contains_matches_the_residue_math() {
        let sieve = Sieve::parse("5@2").unwrap();
        for key in 0..=127u8 {
            assert_eq!(sieve.contains(key), key % 5 == 2, "key {key}");
        }
    }

    #[test]
    fn nearest_ties_break_downward() {
        let sieve = Sieve::parse("12@0").unwrap();
        assert_eq!(sieve.nearest(0), Some(0));
        assert_eq!(sieve.nearest(5), Some(0));
        // 6 is equidistant from 0 and 12: downward wins.
        assert_eq!(sieve.nearest(6), Some(0));
        assert_eq!(sieve.nearest(7), Some(12));
        assert_eq!(sieve.nearest(127), Some(120));
    }

    #[test]
    fn up_and_down_at_the_edges() {
        // Members 11, 23, ..., 119.
        let sieve = Sieve::parse("12@11").unwrap();
        assert_eq!(sieve.up(11), Some(11));
        assert_eq!(sieve.up(12), Some(23));
        assert_eq!(sieve.up(120), None);
        assert_eq!(sieve.down(10), None);
        assert_eq!(sieve.down(11), Some(11));
        assert_eq!(sieve.down(127), Some(119));
        assert_eq!(sieve.nearest(127), Some(119));
        assert_eq!(sieve.nearest(0), Some(11));
    }
}
