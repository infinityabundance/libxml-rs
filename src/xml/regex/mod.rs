//! libxml2's internal regex engine (§85 Phase 7).
//!
//! libxml2 uses its own regex engine (xmlregexp.c) for XML Schema pattern
//! facets, XSLT template match patterns, and other internal uses.
//! Must match upstream behavior exactly.
//!
//! Implements an NFA-based regex engine using Thompson's construction:
//! - Compilation: regex pattern → NFA
//! - Execution: NFA simulation with state-set tracking
//! - Incremental matching: push strings into an execution context
//! - Determinism check

#![allow(
    missing_docs,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals
)]

use core::ffi::c_void;
use core::ptr;
use std::os::raw::c_int;

use crate::abi::allocator::{xmlFreeImpl, xmlMallocImpl};
use crate::abi::types::xmlChar;

// ═══════════════════════════════════════════════════════════════════════════════
// Constants
// ═══════════════════════════════════════════════════════════════════════════════

/// Maximum number of states in an NFA.
#[allow(dead_code)]
const MAX_NFA_STATES: usize = 1024;

/// Maximum recursion depth for parsing.
#[allow(dead_code)]
const MAX_PARSE_DEPTH: usize = 256;

/// Return value for a successful match.
const REGEXP_MATCH: c_int = 1;

/// Return value for no match.
const REGEXP_NOMATCH: c_int = 0;

/// Return value for an error.
const REGEXP_ERROR: c_int = -1;

// ═══════════════════════════════════════════════════════════════════════════════
// Transition Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Type of anchor in a transition.
#[derive(Debug, Clone, Copy, PartialEq)]
enum AnchorType {
    /// Start-of-string anchor `^`
    Start,
    /// End-of-string anchor `$`
    End,
}

/// Character class categories for predefined character classes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PredefinedClass {
    /// `\d` — digit [0-9]
    Digit,
    /// `\D` — non-digit
    NotDigit,
    /// `\s` — whitespace
    Space,
    /// `\S` — non-whitespace
    NotSpace,
    /// `\w` — word character [A-Za-z0-9_]
    Word,
    /// `\W` — non-word character
    NotWord,
}

/// A transition in the NFA.
#[derive(Debug, Clone, PartialEq)]
enum Transition {
    /// Match a specific character.
    Char(u8),
    /// Match any character in a range [lo, hi].
    Range(u8, u8),
    /// Match any character in a set.
    #[allow(dead_code)]
    Set(Vec<u8>),
    /// Match any character NOT in a range.
    NotRange(u8, u8),
    /// Match any character NOT in a set.
    NotSet(Vec<u8>),
    /// Match any character (`.` wildcard).
    Wildcard,
    /// Match a predefined character class.
    Predefined(PredefinedClass),
    /// Epsilon transition (consumes no input).
    Epsilon,
    /// Anchor transition (^ or $).
    Anchor(AnchorType),
}

// ═══════════════════════════════════════════════════════════════════════════════
// NFA Types
// ═══════════════════════════════════════════════════════════════════════════════

/// A single state in the NFA.
#[derive(Debug, Clone)]
struct NfaState {
    /// Transitions from this state.
    transitions: Vec<(Transition, usize)>,
    /// Whether this is an accepting state.
    is_accept: bool,
}

impl NfaState {
    const fn new() -> Self {
        NfaState {
            transitions: Vec::new(),
            is_accept: false,
        }
    }
}

/// A non-deterministic finite automaton.
#[derive(Debug, Clone)]
struct Nfa {
    /// All states in the NFA.
    states: Vec<NfaState>,
    /// The start state index.
    start: usize,
}

impl Nfa {
    fn new() -> Self {
        let start = 0;
        Nfa {
            states: vec![NfaState::new()],
            start,
        }
    }

    /// Add a new state to the NFA and return its index.
    fn add_state(&mut self) -> usize {
        let index = self.states.len();
        self.states.push(NfaState::new());
        index
    }

    /// Add a transition between two states.
    fn add_transition(&mut self, from: usize, to: usize, trans: Transition) {
        if from < self.states.len() && to < self.states.len() {
            self.states[from].transitions.push((trans, to));
        }
    }

    /// Set a state as accepting.
    fn set_accept(&mut self, state: usize) {
        if state < self.states.len() {
            self.states[state].is_accept = true;
        }
    }
}

/// A fragment of an NFA during Thompson construction.
///
/// Tracks the fragment's NFA, its start state, and its "dangling" out states
/// that need to be connected to the next fragment.
struct NfaFragment {
    nfa: Nfa,
    /// The start state of this fragment.
    start: usize,
    /// Set of states that are "dangling" — they should be connected to the
    /// next fragment in a concatenation, or become accepting in the final NFA.
    /// These are the states that currently have no outgoing transitions to
    /// the rest of the NFA.
    out: Vec<usize>,
}

impl NfaFragment {
    const fn new(nfa: Nfa, start: usize, out: Vec<usize>) -> Self {
        NfaFragment { nfa, start, out }
    }
}

impl Clone for NfaFragment {
    fn clone(&self) -> Self {
        NfaFragment {
            nfa: self.nfa.clone(),
            start: self.start,
            out: self.out.clone(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// NFA Construction Primitives
// ═══════════════════════════════════════════════════════════════════════════════

/// Create an NFA fragment matching a single character.
fn nfa_char(c: u8) -> NfaFragment {
    let mut nfa = Nfa::new();
    let start = nfa.start;
    let accept = nfa.add_state();
    nfa.set_accept(accept);
    nfa.add_transition(start, accept, Transition::Char(c));
    NfaFragment::new(nfa, start, vec![accept])
}

/// Create an NFA fragment matching an epsilon (empty string).
fn nfa_epsilon() -> NfaFragment {
    let mut nfa = Nfa::new();
    let start = nfa.start;
    nfa.set_accept(start);
    NfaFragment::new(nfa, start, vec![start])
}

/// Create an NFA fragment matching a character range.
fn nfa_range(lo: u8, hi: u8) -> NfaFragment {
    let mut nfa = Nfa::new();
    let start = nfa.start;
    let accept = nfa.add_state();
    nfa.set_accept(accept);
    nfa.add_transition(start, accept, Transition::Range(lo, hi));
    NfaFragment::new(nfa, start, vec![accept])
}

/// Create an NFA fragment matching a character set.
#[allow(dead_code)]
fn nfa_set(chars: Vec<u8>) -> NfaFragment {
    let mut nfa = Nfa::new();
    let start = nfa.start;
    let accept = nfa.add_state();
    nfa.set_accept(accept);
    nfa.add_transition(start, accept, Transition::Set(chars));
    NfaFragment::new(nfa, start, vec![accept])
}

/// Create an NFA fragment matching a NOT range.
fn nfa_not_range(lo: u8, hi: u8) -> NfaFragment {
    let mut nfa = Nfa::new();
    let start = nfa.start;
    let accept = nfa.add_state();
    nfa.set_accept(accept);
    nfa.add_transition(start, accept, Transition::NotRange(lo, hi));
    NfaFragment::new(nfa, start, vec![accept])
}

/// Create an NFA fragment matching a NOT set.
#[allow(dead_code)]
fn nfa_not_set(chars: Vec<u8>) -> NfaFragment {
    let mut nfa = Nfa::new();
    let start = nfa.start;
    let accept = nfa.add_state();
    nfa.set_accept(accept);
    nfa.add_transition(start, accept, Transition::NotSet(chars));
    NfaFragment::new(nfa, start, vec![accept])
}

/// Create an NFA fragment matching a wildcard (`.`).
fn nfa_wildcard() -> NfaFragment {
    let mut nfa = Nfa::new();
    let start = nfa.start;
    let accept = nfa.add_state();
    nfa.set_accept(accept);
    nfa.add_transition(start, accept, Transition::Wildcard);
    NfaFragment::new(nfa, start, vec![accept])
}

/// Create an NFA fragment matching a predefined class.
fn nfa_predefined(class: PredefinedClass) -> NfaFragment {
    let mut nfa = Nfa::new();
    let start = nfa.start;
    let accept = nfa.add_state();
    nfa.set_accept(accept);
    nfa.add_transition(start, accept, Transition::Predefined(class));
    NfaFragment::new(nfa, start, vec![accept])
}

/// Create an NFA fragment for a start anchor `^`.
fn nfa_start_anchor() -> NfaFragment {
    let mut nfa = Nfa::new();
    let start = nfa.start;
    let accept = nfa.add_state();
    nfa.set_accept(accept);
    nfa.add_transition(start, accept, Transition::Anchor(AnchorType::Start));
    NfaFragment::new(nfa, start, vec![accept])
}

/// Create an NFA fragment for an end anchor `$`.
fn nfa_end_anchor() -> NfaFragment {
    let mut nfa = Nfa::new();
    let start = nfa.start;
    let accept = nfa.add_state();
    nfa.set_accept(accept);
    nfa.add_transition(start, accept, Transition::Anchor(AnchorType::End));
    NfaFragment::new(nfa, start, vec![accept])
}

/// Concatenate two NFA fragments: `a` followed by `b`.
///
/// Connects all dangling out states of `a` to the start state of `b`
/// via epsilon transitions.
fn concat(a: NfaFragment, b: NfaFragment) -> NfaFragment {
    let a_out = a.out.clone();
    let a_start = a.start;
    let a_size = a.nfa.states.len();

    let mut nfa = a.nfa;
    let b_start = b.nfa.start + a_size;

    // Adjust state indices in b's transitions and add b's states
    for mut state in b.nfa.states {
        for (_, target) in &mut state.transitions {
            *target += a_size;
        }
        nfa.states.push(state);
    }

    // Connect a's out states to b's start via epsilon
    for &out_state in &a_out {
        nfa.add_transition(out_state, b_start, Transition::Epsilon);
    }

    // The out states of the concatenation are b's out states (with adjusted indices)
    let b_out: Vec<usize> = b.out.iter().map(|&s| s + a_size).collect();

    NfaFragment::new(nfa, a_start, b_out)
}

/// Union (alternation) of two NFA fragments: `a | b`.
///
/// Creates a new start state with epsilon transitions to both a and b.
fn union(a: NfaFragment, b: NfaFragment) -> NfaFragment {
    let mut nfa = Nfa::new();
    let new_start = nfa.start;

    // Add all states from a (adjusting indices)
    let a_start = nfa.states.len();
    let _a_size = a.nfa.states.len();
    for mut state in a.nfa.states {
        for (_, target) in &mut state.transitions {
            *target += a_start;
        }
        nfa.states.push(state);
    }

    // Add all states from b (adjusting indices)
    let b_start = nfa.states.len();
    for mut state in b.nfa.states {
        for (_, target) in &mut state.transitions {
            *target += b_start;
        }
        nfa.states.push(state);
    }

    // Connect new start to a and b starts
    nfa.add_transition(new_start, a_start, Transition::Epsilon);
    nfa.add_transition(new_start, b_start, Transition::Epsilon);

    // Out states are the out states of both a and b (adjusted)
    let mut out = Vec::new();
    for &s in &a.out {
        out.push(s + a_start);
    }
    for &s in &b.out {
        out.push(s + b_start);
    }

    NfaFragment::new(nfa, new_start, out)
}

/// Kleene star (zero or more repetitions): `a*`.
fn kleene_star(frag: NfaFragment) -> NfaFragment {
    let mut nfa = Nfa::new();
    let new_start = nfa.start;
    let new_accept = nfa.add_state();

    // Add fragment states (adjusted)
    let frag_start = nfa.states.len();
    let _frag_size = frag.nfa.states.len();
    for mut state in frag.nfa.states {
        for (_, target) in &mut state.transitions {
            *target += frag_start;
        }
        nfa.states.push(state);
    }

    // Epsilon from new_start to both new_accept (zero repetitions) and frag_start
    nfa.add_transition(new_start, new_accept, Transition::Epsilon);
    nfa.add_transition(new_start, frag_start, Transition::Epsilon);

    // Epsilon from frag's out states to both frag_start (loop) and new_accept
    for &s in &frag.out {
        nfa.add_transition(s + frag_start, frag_start, Transition::Epsilon);
        nfa.add_transition(s + frag_start, new_accept, Transition::Epsilon);
    }

    nfa.set_accept(new_accept);
    NfaFragment::new(nfa, new_start, vec![new_accept])
}

/// One or more repetitions: `a+`.
fn plus(frag: NfaFragment) -> NfaFragment {
    let out_orig = frag.out.clone();
    let _frag_start_orig = frag.start;

    let mut nfa = Nfa::new();
    let new_start = nfa.start;

    // Add fragment states (adjusted)
    let frag_start = nfa.states.len();
    for mut state in frag.nfa.states {
        for (_, target) in &mut state.transitions {
            *target += frag_start;
        }
        nfa.states.push(state);
    }

    // Epsilon from new_start to frag_start (must match at least once)
    nfa.add_transition(new_start, frag_start, Transition::Epsilon);

    // Connect frag's out states back to frag's start for additional repetitions
    for &s in &out_orig {
        let adjusted = s + frag_start;
        nfa.add_transition(adjusted, frag_start, Transition::Epsilon);
    }

    // Out states are the adjusted original out states
    let out: Vec<usize> = out_orig.iter().map(|&s| s + frag_start).collect();

    NfaFragment::new(nfa, new_start, out)
}

/// Optional: `a?`.
fn optional(frag: NfaFragment) -> NfaFragment {
    let mut nfa = Nfa::new();
    let new_start = nfa.start;
    let new_accept = nfa.add_state();

    let frag_start = nfa.states.len();
    for mut state in frag.nfa.states {
        for (_, target) in &mut state.transitions {
            *target += frag_start;
        }
        nfa.states.push(state);
    }

    // Epsilon from new_start to both new_accept (skip) and frag_start
    nfa.add_transition(new_start, new_accept, Transition::Epsilon);
    nfa.add_transition(new_start, frag_start, Transition::Epsilon);

    // Epsilon from frag's out to new_accept
    for &s in &frag.out {
        nfa.add_transition(s + frag_start, new_accept, Transition::Epsilon);
    }

    nfa.set_accept(new_accept);
    NfaFragment::new(nfa, new_start, vec![new_accept])
}

// ═══════════════════════════════════════════════════════════════════════════════
// Regex Parser
// ═══════════════════════════════════════════════════════════════════════════════

/// Token types for the regex parser.
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
enum RegexToken {
    /// A literal character
    Char(u8),
    /// Wildcard (`.`)
    Dot,
    /// Start anchor (`^`)
    AnchorStart,
    /// End anchor (`$`)
    AnchorEnd,
    /// Left parenthesis
    LParen,
    /// Right parenthesis
    RParen,
    /// Alternation (`|`)
    Pipe,
    /// Zero or more (`*`)
    Star,
    /// One or more (`+`)
    Plus,
    /// Optional (`?`)
    Question,
    /// Left brace for quantifier
    LBrace,
    /// Right brace for quantifier
    RBrace,
    /// Comma in quantifier
    Comma,
    /// Number in quantifier
    #[allow(dead_code)]
    Number(u32),
    /// Escape sequence
    Escape(u8),
    /// Character class start `[`
    ClassStart,
    /// Character class end `]`
    ClassEnd,
    /// Negation in character class `^`
    ClassNegate,
    /// Range in character class `-`
    ClassRange,
}

/// Regex parser state.
struct RegexParser<'a> {
    /// Input pattern bytes.
    input: &'a [u8],
    /// Current position in input.
    pos: usize,
    /// Lookahead token.
    lookahead: Option<RegexToken>,
}

impl<'a> RegexParser<'a> {
    const fn new(input: &'a [u8]) -> Self {
        RegexParser {
            input,
            pos: 0,
            lookahead: None,
        }
    }

    /// Peek at the next character without consuming it.
    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    /// Advance and return the next character.
    fn advance(&mut self) -> Option<u8> {
        let ch = self.input.get(self.pos).copied();
        if ch.is_some() {
            self.pos += 1;
        }
        ch
    }

    /// Skip whitespace in the pattern.
    #[allow(dead_code)]
    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == b' ' || ch == b'\t' || ch == b'\n' || ch == b'\r' {
                self.advance();
            } else {
                break;
            }
        }
    }

    /// Parse the next token and store it as lookahead.
    fn scan_token(&mut self) -> Option<RegexToken> {
        let ch = self.advance()?;
        match ch {
            b'.' => Some(RegexToken::Dot),
            b'^' => Some(RegexToken::AnchorStart),
            b'$' => Some(RegexToken::AnchorEnd),
            b'(' => Some(RegexToken::LParen),
            b')' => Some(RegexToken::RParen),
            b'|' => Some(RegexToken::Pipe),
            b'*' => Some(RegexToken::Star),
            b'+' => Some(RegexToken::Plus),
            b'?' => Some(RegexToken::Question),
            b'{' => Some(RegexToken::LBrace),
            b'}' => Some(RegexToken::RBrace),
            b',' => Some(RegexToken::Comma),
            b'[' => Some(RegexToken::ClassStart),
            b']' => Some(RegexToken::ClassEnd),
            b'\\' => {
                // Escape sequence
                let next = self.advance()?;
                Some(RegexToken::Escape(next))
            }
            _ => Some(RegexToken::Char(ch)),
        }
    }

    /// Get the next token (from lookahead or by scanning).
    fn next_token(&mut self) -> Option<RegexToken> {
        if let Some(token) = self.lookahead.take() {
            Some(token)
        } else {
            self.scan_token()
        }
    }

    /// Push back a token as lookahead.
    const fn unscan(&mut self, token: RegexToken) {
        self.lookahead = Some(token);
    }

    // ── Parsing ──────────────────────────────────────────────────────────

    /// Parse the entire pattern into an NFA fragment.
    fn parse(&mut self) -> Result<NfaFragment, String> {
        let frag = self.parse_alternation()?;
        Ok(frag)
    }

    /// Parse alternation: `expr | expr | ...`
    fn parse_alternation(&mut self) -> Result<NfaFragment, String> {
        let mut frag = self.parse_sequence()?;

        loop {
            match self.next_token() {
                Some(RegexToken::Pipe) => {
                    let rhs = self.parse_sequence()?;
                    frag = union(frag, rhs);
                }
                Some(other) => {
                    self.unscan(other);
                    break;
                }
                None => break,
            }
        }

        Ok(frag)
    }

    /// Parse a sequence of quantified atoms.
    fn parse_sequence(&mut self) -> Result<NfaFragment, String> {
        let mut fragments: Vec<NfaFragment> = Vec::new();

        loop {
            match self.peek() {
                None => break,
                Some(b'|') | Some(b')') => break,
                _ => {}
            }

            let atom = self.parse_atom()?;
            fragments.push(atom);
        }

        if fragments.is_empty() {
            return Ok(nfa_epsilon());
        }

        let mut result = fragments.remove(0);
        for frag in fragments {
            result = concat(result, frag);
        }

        Ok(result)
    }

    /// Parse an atom (possibly with quantifier).
    fn parse_atom(&mut self) -> Result<NfaFragment, String> {
        let token = self
            .next_token()
            .ok_or_else(|| "Unexpected end of pattern".to_string())?;

        let base = match token {
            RegexToken::Char(c) => nfa_char(c),
            RegexToken::Dot => nfa_wildcard(),
            RegexToken::AnchorStart => nfa_start_anchor(),
            RegexToken::AnchorEnd => nfa_end_anchor(),
            RegexToken::LParen => {
                let inner = self.parse_alternation()?;
                match self.next_token() {
                    Some(RegexToken::RParen) => inner,
                    Some(t) => return Err(format!("Expected ')', got {:?}", t)),
                    None => return Err("Unterminated group".to_string()),
                }
            }
            RegexToken::Escape(c) => {
                match c {
                    b'd' => nfa_predefined(PredefinedClass::Digit),
                    b'D' => nfa_predefined(PredefinedClass::NotDigit),
                    b's' => nfa_predefined(PredefinedClass::Space),
                    b'S' => nfa_predefined(PredefinedClass::NotSpace),
                    b'w' => nfa_predefined(PredefinedClass::Word),
                    b'W' => nfa_predefined(PredefinedClass::NotWord),
                    b'n' => nfa_char(b'\n'),
                    b'r' => nfa_char(b'\r'),
                    b't' => nfa_char(b'\t'),
                    b'\\' => nfa_char(b'\\'),
                    b'.' => nfa_char(b'.'),
                    b'^' => nfa_char(b'^'),
                    b'$' => nfa_char(b'$'),
                    b'|' => nfa_char(b'|'),
                    b'*' => nfa_char(b'*'),
                    b'+' => nfa_char(b'+'),
                    b'?' => nfa_char(b'?'),
                    b'(' => nfa_char(b'('),
                    b')' => nfa_char(b')'),
                    b'[' => nfa_char(b'['),
                    b']' => nfa_char(b']'),
                    b'{' => nfa_char(b'{'),
                    b'}' => nfa_char(b'}'),
                    b'-' => nfa_char(b'-'),
                    b'0'..=b'9' => {
                        // Backreference or octal - treat as literal for now
                        nfa_char(c)
                    }
                    _ => nfa_char(c),
                }
            }
            RegexToken::ClassStart => self.parse_char_class()?,
            _ => return Err(format!("Unexpected token: {:?}", token)),
        };

        // Check for quantifier
        self.parse_quantifier(base)
    }

    /// Parse a quantifier after an atom.
    fn parse_quantifier(&mut self, frag: NfaFragment) -> Result<NfaFragment, String> {
        match self.peek() {
            Some(b'*') => {
                self.advance();
                Ok(kleene_star(frag))
            }
            Some(b'+') => {
                self.advance();
                Ok(plus(frag))
            }
            Some(b'?') => {
                self.advance();
                Ok(optional(frag))
            }
            Some(b'{') => {
                self.advance();
                self.parse_brace_quantifier(frag)
            }
            _ => Ok(frag),
        }
    }

    /// Parse a brace quantifier `{n}`, `{n,}`, or `{n,m}`.
    fn parse_brace_quantifier(&mut self, frag: NfaFragment) -> Result<NfaFragment, String> {
        // Parse minimum
        let mut min: u32 = 0;
        while let Some(b'0'..=b'9') = self.peek() {
            let d = self.advance().unwrap() - b'0';
            min = min * 10 + d as u32;
        }

        let mut max: Option<u32> = None;

        match self.peek() {
            Some(b',') => {
                self.advance();
                // Parse maximum
                let mut max_val: u32 = 0;
                let mut has_max = false;
                while let Some(b'0'..=b'9') = self.peek() {
                    let d = self.advance().unwrap() - b'0';
                    max_val = max_val * 10 + d as u32;
                    has_max = true;
                }
                if has_max {
                    max = Some(max_val);
                }
            }
            Some(b'}') => {
                max = Some(min);
            }
            _ => return Err("Expected '}' in quantifier".to_string()),
        }

        // Expect closing brace
        match self.peek() {
            Some(b'}') => {
                self.advance();
            }
            _ => return Err("Expected '}' in quantifier".to_string()),
        }

        // Build the quantifier NFA
        // {n} = exactly n repetitions
        // {n,} = at least n repetitions
        // {n,m} = between n and m repetitions
        if min == 0 && max.is_none() {
            // {0,} = *
            Ok(kleene_star(frag))
        } else if min == 0 && max == Some(0) {
            // {0} = empty
            Ok(nfa_epsilon())
        } else if min == 1 && max.is_none() {
            // {1,} = +
            Ok(plus(frag))
        } else if min == 0 && max == Some(1) {
            // {0,1} = ?
            Ok(optional(frag))
        } else {
            // General case: build concatenation of min repetitions,
            // plus optional repetitions for max
            let mut result = nfa_epsilon();
            for _ in 0..min {
                result = concat(result, frag.clone());
            }
            if let Some(max_val) = max {
                for _ in min..max_val {
                    result = concat(result, optional(frag.clone()));
                }
            } else {
                // At least min, then zero or more
                result = concat(result, kleene_star(frag.clone()));
            }
            Ok(result)
        }
    }

    /// Parse a character class `[...]` or `[^...]`.
    fn parse_char_class(&mut self) -> Result<NfaFragment, String> {
        let mut chars: Vec<u8> = Vec::new();
        let mut ranges: Vec<(u8, u8)> = Vec::new();
        let mut negated = false;

        // Check for negation
        if let Some(b'^') = self.peek() {
            negated = true;
            self.advance();
        }

        // Parse character class contents
        let mut prev: Option<u8> = None;

        loop {
            match self.peek() {
                None => return Err("Unterminated character class".to_string()),
                Some(b']') => {
                    if prev.is_some() {
                        // Treat '-' before ']' as literal
                        if let Some(c) = prev {
                            chars.push(c);
                        }
                        prev = None;
                    }
                    self.advance();
                    break;
                }
                Some(b'-') if prev.is_some() => {
                    // Range operator
                    self.advance();
                    let lo = prev.take().unwrap();
                    match self.peek() {
                        Some(b']') => {
                            // '-' before ']' is literal
                            chars.push(lo);
                            chars.push(b'-');
                            prev = None;
                        }
                        Some(ch) => {
                            self.advance();
                            if lo <= ch {
                                ranges.push((lo, ch));
                            }
                            prev = None;
                        }
                        None => {
                            chars.push(lo);
                            chars.push(b'-');
                            prev = None;
                        }
                    }
                }
                Some(b'\\') => {
                    self.advance();
                    if let Some(ch) = self.advance() {
                        // UPSTREAM-PARITY: Inside character classes, \d, \D, \s, \S,
                        // \w, \W create predefined class transitions rather than
                        // literal character matches.
                        match ch {
                            b'd' | b'D' | b's' | b'S' | b'w' | b'W' => {
                                // Push any pending prev first
                                if let Some(p) = prev.take() {
                                    chars.push(p);
                                }
                                // We can't directly return a predefined fragment here
                                // because we're in the middle of parsing. Instead, convert
                                // the predefined class to its equivalent range/chars.
                                let class = match ch {
                                    b'd' => PredefinedClass::Digit,
                                    b'D' => PredefinedClass::NotDigit,
                                    b's' => PredefinedClass::Space,
                                    b'S' => PredefinedClass::NotSpace,
                                    b'w' => PredefinedClass::Word,
                                    b'W' => PredefinedClass::NotWord,
                                    _ => unreachable!(),
                                };
                                // Add the predefined class equivalent ranges
                                // We'll handle this after the loop
                                // Store a sentinel: we'll push special entries
                                // For now, just add digit ranges
                                match class {
                                    PredefinedClass::Digit => {
                                        ranges.push((b'0', b'9'));
                                    }
                                    PredefinedClass::NotDigit => {
                                        ranges.push((0x00, b'/' - 1));
                                        ranges.push((b':', 0xFF));
                                    }
                                    PredefinedClass::Space => {
                                        chars.push(b' ');
                                        chars.push(b'\t');
                                        chars.push(b'\n');
                                        chars.push(b'\r');
                                    }
                                    PredefinedClass::NotSpace => {
                                        // Everything except space, tab, newline, carriage return
                                        ranges.push((0x00, b' ' - 1));
                                        ranges.push((b'!' + 1, b'\t' - 1));
                                        ranges.push((b'\t' + 1, b'\n' - 1));
                                        ranges.push((b'\n' + 1, b'\r' - 1));
                                        ranges.push((b'\r' + 1, 0xFF));
                                    }
                                    PredefinedClass::Word => {
                                        ranges.push((b'0', b'9'));
                                        ranges.push((b'A', b'Z'));
                                        ranges.push((b'a', b'z'));
                                        chars.push(b'_');
                                    }
                                    PredefinedClass::NotWord => {
                                        ranges.push((0x00, b'0' - 1));
                                        ranges.push((b'9' + 1, b'A' - 1));
                                        ranges.push((b'Z' + 1, b'_' - 1));
                                        ranges.push((b'_' + 1, b'a' - 1));
                                        ranges.push((b'z' + 1, 0xFF));
                                    }
                                }
                            }
                            _ => {
                                let c = match ch {
                                    b'n' => b'\n',
                                    b'r' => b'\r',
                                    b't' => b'\t',
                                    b'\\' => b'\\',
                                    b'0'..=b'9' => ch, // treat as literal
                                    _ => ch,
                                };
                                if let Some(p) = prev.take() {
                                    chars.push(p);
                                }
                                prev = Some(c);
                            }
                        }
                    }
                }
                Some(ch) => {
                    self.advance();
                    if let Some(p) = prev.take() {
                        chars.push(p);
                    }
                    prev = Some(ch);
                }
            }
        }

        // Push any remaining prev
        if let Some(c) = prev {
            chars.push(c);
        }

        // Build the NFA fragment
        let mut combined_ranges: Vec<(u8, u8)> = ranges;
        for &c in &chars {
            combined_ranges.push((c, c));
        }

        if combined_ranges.is_empty() {
            return if negated {
                // [^] matches nothing? Actually [^] is invalid in XML regex
                Ok(nfa_epsilon())
            } else {
                // [] is invalid, treat as empty
                Ok(nfa_epsilon())
            };
        }

        // Merge overlapping/consecutive ranges
        combined_ranges.sort_by_key(|a| a.0);
        let mut merged: Vec<(u8, u8)> = Vec::new();
        for (lo, hi) in combined_ranges {
            if let Some(last) = merged.last_mut() {
                if lo <= last.1 + 1 {
                    last.1 = last.1.max(hi);
                    continue;
                }
            }
            merged.push((lo, hi));
        }

        // Build fragment from merged ranges
        if negated {
            if merged.len() == 1 && merged[0].0 == 0 && merged[0].1 == 255 {
                // [^...] with everything is impossible — empty
                Ok(nfa_epsilon())
            } else {
                Ok(nfa_not_range(0, 255))
            }
        } else if merged.len() == 1 {
            let (lo, hi) = merged[0];
            if lo == hi {
                Ok(nfa_char(lo))
            } else {
                Ok(nfa_range(lo, hi))
            }
        } else {
            // Multiple ranges: union them
            let mut result = nfa_range(merged[0].0, merged[0].1);
            for &(lo, hi) in &merged[1..] {
                result = union(result, nfa_range(lo, hi));
            }
            Ok(result)
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// NFA Simulation — Character Class Matching
// ═══════════════════════════════════════════════════════════════════════════════

/// Check if a byte matches a given transition.
fn matches_transition(c: u8, trans: &Transition) -> bool {
    match trans {
        Transition::Char(ch) => c == *ch,
        Transition::Range(lo, hi) => c >= *lo && c <= *hi,
        Transition::Set(chars) => chars.contains(&c),
        Transition::NotRange(lo, hi) => c < *lo || c > *hi,
        Transition::NotSet(chars) => !chars.contains(&c),
        Transition::Wildcard => true,
        Transition::Predefined(class) => matches_predefined(c, *class),
        Transition::Epsilon => false,   // Epsilon handled separately
        Transition::Anchor(_) => false, // Anchors handled separately
    }
}

/// Check if a character matches a predefined class.
const fn matches_predefined(c: u8, class: PredefinedClass) -> bool {
    match class {
        PredefinedClass::Digit => c >= b'0' && c <= b'9',
        PredefinedClass::NotDigit => c < b'0' || c > b'9',
        PredefinedClass::Space => {
            c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' || c == 0x0b || c == 0x0c
        }
        PredefinedClass::NotSpace => {
            !(c == b' ' || c == b'\t' || c == b'\n' || c == b'\r' || c == 0x0b || c == 0x0c)
        }
        PredefinedClass::Word => {
            (c >= b'0' && c <= b'9')
                || (c >= b'A' && c <= b'Z')
                || (c >= b'a' && c <= b'z')
                || c == b'_'
        }
        PredefinedClass::NotWord => {
            !((c >= b'0' && c <= b'9')
                || (c >= b'A' && c <= b'Z')
                || (c >= b'a' && c <= b'z')
                || c == b'_')
        }
    }
}

/// Compute the epsilon closure of a set of states.
fn epsilon_closure(nfa: &Nfa, states: &[usize]) -> Vec<usize> {
    let mut visited = vec![false; nfa.states.len()];
    let mut result = Vec::new();
    let mut stack: Vec<usize> = states.to_vec();

    while let Some(s) = stack.pop() {
        if s >= nfa.states.len() || visited[s] {
            continue;
        }
        visited[s] = true;
        result.push(s);

        for (cond, target) in &nfa.states[s].transitions {
            if matches!(cond, Transition::Epsilon)
                && *target < nfa.states.len()
                && !visited[*target]
            {
                stack.push(*target);
            }
        }
    }

    result
}

/// Move from a set of states on a single character.
///
/// Returns all states reachable from any state in `states` by consuming `c`.
/// First follows non-consuming transitions (anchors), then character transitions.
fn move_on_char(nfa: &Nfa, states: &[usize], c: u8, is_start: bool, is_end: bool) -> Vec<usize> {
    // Step 1: Follow non-consuming transitions (anchors) to expand
    // the set of states, without consuming the character.
    let mut expanded = states.to_vec();
    let mut more = true;
    while more {
        more = false;
        let mut new_states = Vec::new();
        for &s in &expanded {
            if s >= nfa.states.len() {
                continue;
            }
            for (cond, target) in &nfa.states[s].transitions {
                if *target >= nfa.states.len() {
                    continue;
                }
                let should_follow = match cond {
                    Transition::Anchor(AnchorType::Start) => {
                        is_start && !expanded.contains(target) && !new_states.contains(target)
                    }
                    Transition::Anchor(AnchorType::End) => {
                        is_end && !expanded.contains(target) && !new_states.contains(target)
                    }
                    _ => false,
                };
                if should_follow {
                    new_states.push(*target);
                    more = true;
                }
            }
        }
        expanded.extend(new_states);
    }

    // Step 1.5: Follow epsilon transitions from the expanded set.
    // Anchor states may connect to character-consuming states via epsilon
    // (introduced by concat()). We must traverse these before consuming input.
    let mut eps_expanded = expanded.clone();
    let mut more_eps = true;
    while more_eps {
        more_eps = false;
        let mut new_eps = Vec::new();
        for &s in &eps_expanded {
            if s >= nfa.states.len() {
                continue;
            }
            for (cond, target) in &nfa.states[s].transitions {
                if let Transition::Epsilon = cond {
                    if *target < nfa.states.len()
                        && !eps_expanded.contains(target)
                        && !new_eps.contains(target)
                    {
                        new_eps.push(*target);
                        more_eps = true;
                    }
                }
            }
        }
        eps_expanded.extend(new_eps);
    }

    // Step 2: From the fully expanded set, follow character-consuming transitions.
    let mut next = Vec::new();
    for &s in &eps_expanded {
        if s >= nfa.states.len() {
            continue;
        }
        for (cond, target) in &nfa.states[s].transitions {
            if *target >= nfa.states.len() {
                continue;
            }
            match cond {
                Transition::Epsilon => continue,
                Transition::Anchor(_) => continue, // already handled above
                _ => {
                    if matches_transition(c, cond) && !next.contains(target) {
                        next.push(*target);
                    }
                }
            }
        }
    }

    next
}

/// Check if any state in the set is an accepting state.
fn has_accept_state(nfa: &Nfa, states: &[usize]) -> bool {
    states
        .iter()
        .any(|&s| s < nfa.states.len() && nfa.states[s].is_accept)
}

/// Execute an NFA against a byte string (full match).
///
/// Returns `REGEXP_MATCH` (1) if the entire string matches,
/// `REGEXP_NOMATCH` (0) if it doesn't, or `REGEXP_ERROR` (-1) on error.
fn nfa_exec(nfa: &Nfa, input: &[u8]) -> c_int {
    if nfa.states.is_empty() {
        return REGEXP_ERROR;
    }

    // Start with epsilon closure of the start state.
    let mut current = epsilon_closure(nfa, &[nfa.start]);

    // If input is empty, check immediately if we're in an accept state.
    // Also handle end anchor ($), which matches the end of input (empty string at end).
    if input.is_empty() {
        let mut final_states = current.clone();
        for &s in &current {
            for (cond, target) in &nfa.states[s].transitions {
                if let Transition::Anchor(AnchorType::End) = cond {
                    if *target < nfa.states.len() {
                        let ec = epsilon_closure(nfa, &[*target]);
                        final_states.extend(ec);
                    }
                }
            }
        }
        final_states = epsilon_closure(nfa, &final_states);
        return if has_accept_state(nfa, &final_states) {
            REGEXP_MATCH
        } else {
            REGEXP_NOMATCH
        };
    }

    for (i, &c) in input.iter().enumerate() {
        let is_start = i == 0;
        // Move on this character. move_on_char handles both character
        // transitions and anchor transitions (^ at start, $ at end).
        let next_states = move_on_char(nfa, &current, c, is_start, false);
        if next_states.is_empty() {
            return REGEXP_NOMATCH;
        }

        current = epsilon_closure(nfa, &next_states);
        if current.is_empty() {
            return REGEXP_NOMATCH;
        }
    }

    // After consuming all input, check if we can reach an accept state.
    // Also handle end anchor ($) by checking if any state can transition
    // to an accept state via end anchor.
    let mut final_states = current.clone();
    for &s in &current {
        for (cond, target) in &nfa.states[s].transitions {
            if let Transition::Anchor(AnchorType::End) = cond {
                if *target < nfa.states.len() {
                    let ec = epsilon_closure(nfa, &[*target]);
                    final_states.extend(ec);
                }
            }
        }
    }
    final_states = epsilon_closure(nfa, &final_states);

    if has_accept_state(nfa, &final_states) {
        REGEXP_MATCH
    } else {
        REGEXP_NOMATCH
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// XmlRegexp — Compiled Regex Type
// ═══════════════════════════════════════════════════════════════════════════════

/// Compiled regular expression.
///
/// # UPSTREAM-PARITY
///
/// Corresponds to `xmlRegexpPtr` / `_xmlRegexp` in libxml2.
#[derive(Debug)]
#[repr(C)]
pub struct XmlRegexp {
    /// The original pattern string (null-terminated xmlChar*).
    pattern: *mut xmlChar,
    /// The internal NFA representation.
    nfa: Option<Box<Nfa>>,
    /// Whether the regex is deterministic.
    is_deterministic: c_int,
}

/// Compile a regex pattern into an NFA.
fn compile_pattern(pattern: &[u8]) -> Option<Box<Nfa>> {
    if pattern.is_empty() {
        // Empty pattern matches empty string
        let mut nfa = Nfa::new();
        nfa.set_accept(nfa.start);
        return Some(Box::new(nfa));
    }

    let mut parser = RegexParser::new(pattern);
    match parser.parse() {
        Ok(fragment) => {
            // UPSTREAM-PARITY: Clear ALL accept flags first, then set only the
            // fragment's final out states as accepting. This ensures that inner
            // fragment accept states (from kleene_star, plus, etc.) are not
            // incorrectly treated as accepting when they are connected to
            // subsequent fragments via concatenation.
            let mut nfa = fragment.nfa;
            for state in &mut nfa.states {
                state.is_accept = false;
            }
            for &s in &fragment.out {
                if s < nfa.states.len() {
                    nfa.set_accept(s);
                }
            }
            Some(Box::new(nfa))
        }
        Err(_) => None,
    }
}

/// Check if an NFA is deterministic.
fn is_deterministic(nfa: &Nfa) -> bool {
    // UPSTREAM-PARITY: An NFA is deterministic if, for every state, there is
    // at most one transition that can match any given input character.
    //
    // Key rules:
    // 1. Epsilon transitions from concatenation are fine (they chain linearly).
    // 2. Multiple epsilon transitions from the same state (alternation) make
    //    it non-deterministic.
    // 3. An epsilon + character-consuming transition from the same state
    //    makes it non-deterministic (engine must choose).
    // 4. Overlapping character-consuming transitions (e.g., 'a' and range('a','z'))
    //    make it non-deterministic.
    //
    // We check each state individually. For states with only epsilon transitions,
    // we check if there's exactly one (which is fine, it's a pass-through from
    // concatenation) or multiple (which is non-deterministic from alternation).
    //
    // For states with non-epsilon transitions, we check for overlap.
    // We also consider the epsilon-closure: if a state has an epsilon to another
    // state, the character-consuming transitions of both states must not overlap.

    for (state_idx, state) in nfa.states.iter().enumerate() {
        // Count epsilon transitions from this state directly.
        let epsilon_count = state
            .transitions
            .iter()
            .filter(|(cond, _)| matches!(cond, Transition::Epsilon))
            .count();

        // Count non-epsilon, non-anchor transitions.
        let consuming_count = state
            .transitions
            .iter()
            .filter(|(cond, _)| !matches!(cond, Transition::Epsilon | Transition::Anchor(_)))
            .count();

        // Rule 2: Multiple epsilon transitions from same state = non-deterministic.
        if epsilon_count > 1 {
            return false;
        }

        // Rule 3: Epsilon + consuming from same state = non-deterministic.
        if epsilon_count > 0 && consuming_count > 0 {
            return false;
        }

        // Collect all consuming transitions (direct + via epsilon closure).
        // But only if there are no epsilon transitions (otherwise already caught).
        let closure = if epsilon_count == 0 {
            epsilon_closure(nfa, &[state_idx])
        } else {
            // Already handled above; skip detailed check.
            continue;
        };

        let mut has_wildcard = false;
        let mut chars = Vec::new();
        let mut has_range = false;
        let mut has_not = false;
        let mut has_predefined = false;

        for &s_idx in &closure {
            if s_idx >= nfa.states.len() {
                continue;
            }
            for (cond, _) in &nfa.states[s_idx].transitions {
                match cond {
                    Transition::Epsilon | Transition::Anchor(_) => {}
                    Transition::Wildcard => {
                        if has_wildcard {
                            return false;
                        }
                        has_wildcard = true;
                    }
                    Transition::Char(c) => {
                        chars.push(*c);
                    }
                    Transition::Range(_, _) => {
                        has_range = true;
                    }
                    Transition::Set(_) => {}
                    Transition::NotRange(_, _) | Transition::NotSet(_) => {
                        has_not = true;
                    }
                    Transition::Predefined(_) => {
                        has_predefined = true;
                    }
                }
            }
        }

        // Rule 4: Check for overlapping character-consuming transitions.
        if has_wildcard && (!chars.is_empty() || has_range || has_not || has_predefined) {
            return false;
        }

        if has_range && (has_wildcard || has_not || has_predefined) {
            return false;
        }

        if has_not && (has_wildcard || has_range || has_predefined) {
            return false;
        }

        if has_predefined && (has_wildcard || has_range || has_not) {
            return false;
        }

        // Check for duplicate characters.
        chars.sort();
        let dedup_len = {
            chars.dedup();
            chars.len()
        };
        // If we had chars and they deduplicated to fewer, there were duplicates.
        // But actually the issue is: multiple chars on same state are fine
        // (each is a distinct transition), but a single char appearing twice
        // would indicate non-determinism. Since we already dedup'd, if length
        // before dedup > length after dedup, there were duplicates.
        // We don't have the original length here easily.
        // Instead: if we have multiple distinct chars AND any other type,
        // it's non-deterministic.
        if dedup_len > 1 && (has_range || has_wildcard || has_not || has_predefined) {
            return false;
        }
    }

    true
}

// ═══════════════════════════════════════════════════════════════════════════════
// C ABI Functions
// ═══════════════════════════════════════════════════════════════════════════════

/// xmlChar* helper: get length of null-terminated xmlChar string.
const unsafe fn xml_strlen(s: *const xmlChar) -> usize {
    if s.is_null() {
        return 0;
    }
    let mut len: usize = 0;
    while *s.add(len) != 0 {
        len += 1;
    }
    len
}

/// xmlChar* helper: duplicate a null-terminated xmlChar string.
unsafe fn xml_strdup(s: *const xmlChar) -> *mut xmlChar {
    if s.is_null() {
        return ptr::null_mut();
    }
    let len = xml_strlen(s);
    let new_ptr = xmlMallocImpl((len + 1) * core::mem::size_of::<xmlChar>()) as *mut xmlChar;
    if new_ptr.is_null() {
        return ptr::null_mut();
    }
    for i in 0..=len {
        unsafe { *new_ptr.add(i) = *s.add(i) };
    }
    new_ptr
}

/// Compile a regex pattern.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlRegexpPtr xmlRegexpCompile(const xmlChar *pattern);
/// ```
///
/// Returns a compiled regex or NULL on error.
///
/// # SAFETY
///
/// - `pattern` must be a valid null-terminated xmlChar string, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRegexpCompile(pattern: *const xmlChar) -> *mut XmlRegexp {
    if pattern.is_null() {
        return ptr::null_mut();
    }

    let len = xml_strlen(pattern);
    let pattern_bytes = unsafe { core::slice::from_raw_parts(pattern, len) };

    let nfa = compile_pattern(pattern_bytes);
    let pattern_copy = xml_strdup(pattern);

    let compiled = xmlMallocImpl(core::mem::size_of::<XmlRegexp>()) as *mut XmlRegexp;
    if compiled.is_null() {
        if !pattern_copy.is_null() {
            xmlFreeImpl(pattern_copy as *mut c_void);
        }
        return ptr::null_mut();
    }

    let det = nfa
        .as_ref()
        .map_or(0, |n| if is_deterministic(n) { 1 } else { 0 });

    unsafe {
        (*compiled).pattern = pattern_copy;
        // Use ptr::write to avoid dropping uninitialized memory.
        // xmlMalloc returns uninitialized memory; Rust's assignment operator
        // would try to drop the old (garbage) value for fields with Drop.
        core::ptr::write(&mut (*compiled).nfa, nfa);
        (*compiled).is_deterministic = det;
    }

    compiled
}

/// Execute a compiled regex against a string.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlRegexpExec(const xmlRegexpPtr compiled, const xmlChar *value);
/// ```
///
/// Returns 1 if the value matches, 0 if not, -1 on error.
///
/// # SAFETY
///
/// - `compiled` must be a valid pointer to an XmlRegexp, or NULL.
/// - `value` must be a valid null-terminated xmlChar string, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRegexpExec(compiled: *const XmlRegexp, value: *const xmlChar) -> c_int {
    if compiled.is_null() || value.is_null() {
        return REGEXP_ERROR;
    }

    let regex = unsafe { &*compiled };
    let nfa = match &regex.nfa {
        Some(nfa) => nfa,
        None => return REGEXP_ERROR,
    };

    let len = xml_strlen(value);
    let input = unsafe { core::slice::from_raw_parts(value, len) };

    nfa_exec(nfa, input)
}

/// Check if a compiled regex is deterministic.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlRegexpIsDeterministic(const xmlRegexpPtr compiled);
/// ```
///
/// Returns 1 if deterministic, 0 otherwise.
///
/// # SAFETY
///
/// - `compiled` must be a valid pointer to an XmlRegexp, or NULL.
#[no_mangle]
pub const unsafe extern "C" fn xmlRegexpIsDeterministic(compiled: *const XmlRegexp) -> c_int {
    if compiled.is_null() {
        return 0;
    }
    unsafe { (*compiled).is_deterministic }
}

/// Print a compiled regex for debugging.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlRegexpPrint(FILE *output, xmlRegexpPtr compiled);
/// ```
///
/// # SAFETY
///
/// - `compiled` must be a valid pointer to an XmlRegexp, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRegexpPrint(compiled: *const XmlRegexp) {
    if compiled.is_null() {
        eprintln!("(null regex)");
        return;
    }

    let regex = unsafe { &*compiled };
    let pattern_str = if regex.pattern.is_null() {
        "(null)"
    } else {
        let len = xml_strlen(regex.pattern);
        let slice = unsafe { core::slice::from_raw_parts(regex.pattern, len) };
        core::str::from_utf8(slice).unwrap_or("(invalid utf-8)")
    };

    eprintln!("Regex: /{}/", pattern_str);
    eprintln!("  deterministic: {}", regex.is_deterministic);

    if let Some(ref nfa) = regex.nfa {
        eprintln!("  states: {}", nfa.states.len());
        eprintln!("  start state: {}", nfa.start);
        for (i, state) in nfa.states.iter().enumerate() {
            eprint!("    state[{}]: ", i);
            if state.is_accept {
                eprint!("(accept) ");
            }
            for (j, (cond, target)) in state.transitions.iter().enumerate() {
                if j > 0 {
                    eprint!(", ");
                }
                match cond {
                    Transition::Epsilon => eprint!("ε->{}", target),
                    Transition::Char(c) => {
                        if *c >= 0x20 && *c <= 0x7e {
                            eprint!("'{}'->{}", *c as char, target);
                        } else {
                            eprint!("0x{:02x}->{}", c, target);
                        }
                    }
                    Transition::Range(lo, hi) => {
                        eprint!("[{:02x}-{:02x}]->{}", lo, hi, target);
                    }
                    Transition::Set(chars) => {
                        eprint!("{{");
                        for (k, c) in chars.iter().enumerate() {
                            if k > 0 {
                                eprint!(",");
                            }
                            eprint!("0x{:02x}", c);
                        }
                        eprint!("}}->{}", target);
                    }
                    Transition::NotRange(lo, hi) => {
                        eprint!("[^{:02x}-{:02x}]->{}", lo, hi, target);
                    }
                    Transition::NotSet(chars) => {
                        eprint!("^{{");
                        for (k, c) in chars.iter().enumerate() {
                            if k > 0 {
                                eprint!(",");
                            }
                            eprint!("0x{:02x}", c);
                        }
                        eprint!("}}->{}", target);
                    }
                    Transition::Wildcard => eprint!(".*->{}", target),
                    Transition::Predefined(class) => {
                        let name = match class {
                            PredefinedClass::Digit => "\\d",
                            PredefinedClass::NotDigit => "\\D",
                            PredefinedClass::Space => "\\s",
                            PredefinedClass::NotSpace => "\\S",
                            PredefinedClass::Word => "\\w",
                            PredefinedClass::NotWord => "\\W",
                        };
                        eprint!("{}->{}", name, target);
                    }
                    Transition::Anchor(at) => match at {
                        AnchorType::Start => eprint!("^->{}", target),
                        AnchorType::End => eprint!("$->{}", target),
                    },
                }
            }
            eprintln!();
        }
    }
}

/// Free a compiled regex.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlRegFreeRegexp(xmlRegexpPtr regexp);
/// ```
///
/// # SAFETY
///
/// - `regexp` must be a valid pointer to an XmlRegexp previously returned
///   by `xmlRegexpCompile`, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRegFreeRegexp(regexp: *mut XmlRegexp) {
    if regexp.is_null() {
        return;
    }
    unsafe {
        if !(*regexp).pattern.is_null() {
            xmlFreeImpl((*regexp).pattern as *mut c_void);
        }
        // Drop the NFA box
        let _ = (*regexp).nfa.take();
        xmlFreeImpl(regexp as *mut c_void);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// RegExecCtxt — Incremental Regex Execution Context
// ═══════════════════════════════════════════════════════════════════════════════

/// Incremental regex execution context.
///
/// # UPSTREAM-PARITY
///
/// Corresponds to `xmlRegExecCtxtPtr` / `_xmlRegExecCtxt` in libxml2.
#[derive(Debug)]
#[repr(C)]
pub struct RegExecCtxt {
    /// The compiled regex being executed.
    compiled: *mut XmlRegexp,
    /// Current set of NFA states (indices into the NFA).
    current_states: Vec<usize>,
    /// Whether we've started matching.
    started: bool,
}

/// Create an incremental regex execution context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// xmlRegExecCtxtPtr xmlRegNewExecCtxt(xmlRegexpPtr compiled, void *data);
/// ```
///
/// # SAFETY
///
/// - `compiled` must be a valid pointer to an XmlRegexp, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRegNewExecCtxt(
    compiled: *mut XmlRegexp,
    _data: *mut c_void,
) -> *mut RegExecCtxt {
    if compiled.is_null() {
        return ptr::null_mut();
    }

    let ctxt = xmlMallocImpl(core::mem::size_of::<RegExecCtxt>()) as *mut RegExecCtxt;
    if ctxt.is_null() {
        return ptr::null_mut();
    }

    unsafe {
        (*ctxt).compiled = compiled;
        // Use ptr::write to avoid dropping uninitialized memory.
        // xmlMalloc returns uninitialized memory; Rust's assignment operator
        // would try to drop the old (garbage) value for fields with Drop.
        core::ptr::write(&mut (*ctxt).current_states, Vec::new());
        (*ctxt).started = false;
    }

    ctxt
}

/// Push a string into the incremental regex execution context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// int xmlRegExecPushString(xmlRegExecCtxtPtr ctxt, const xmlChar *value);
/// ```
///
/// Returns 1 if the pushed data completes a match, 0 if more data is needed,
/// -1 on error.
///
/// # SAFETY
///
/// - `ctxt` must be a valid pointer to a RegExecCtxt, or NULL.
/// - `value` must be a valid null-terminated xmlChar string, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRegExecPushString(
    ctxt: *mut RegExecCtxt,
    value: *const xmlChar,
) -> c_int {
    if ctxt.is_null() {
        return REGEXP_ERROR;
    }

    let exec_ctxt = unsafe { &mut *ctxt };
    let regex = match unsafe { exec_ctxt.compiled.as_mut() } {
        Some(r) => r,
        None => return REGEXP_ERROR,
    };

    let nfa = match &regex.nfa {
        Some(nfa) => nfa,
        None => return REGEXP_ERROR,
    };

    if value.is_null() {
        // NULL means end of input — check if current state is accepting.
        // If not started yet, initialize from start state first.
        if !exec_ctxt.started {
            exec_ctxt.current_states = epsilon_closure(nfa, &[nfa.start]);
            exec_ctxt.started = true;
        }
        return if has_accept_state(nfa, &exec_ctxt.current_states) {
            REGEXP_MATCH
        } else {
            REGEXP_NOMATCH
        };
    }

    let len = xml_strlen(value);
    let input = unsafe { core::slice::from_raw_parts(value, len) };

    if input.is_empty() {
        return REGEXP_NOMATCH;
    }

    if !exec_ctxt.started {
        // Initialize with epsilon closure of start state
        exec_ctxt.current_states = epsilon_closure(nfa, &[nfa.start]);
        exec_ctxt.started = true;
    }

    for (i, &c) in input.iter().enumerate() {
        let is_end = i == input.len() - 1 && true; // end of this push, but not necessarily end of all input
        let next_states = move_on_char(nfa, &exec_ctxt.current_states, c, i == 0, is_end);
        if next_states.is_empty() {
            exec_ctxt.current_states = Vec::new();
            return REGEXP_NOMATCH;
        }
        exec_ctxt.current_states = epsilon_closure(nfa, &next_states);
    }

    // Check if current state set contains an accept state
    if has_accept_state(nfa, &exec_ctxt.current_states) {
        REGEXP_MATCH
    } else {
        REGEXP_NOMATCH
    }
}

/// Free an incremental regex execution context.
///
/// # UPSTREAM-PARITY
///
/// ```c
/// void xmlRegFreeExecCtxt(xmlRegExecCtxtPtr ctxt);
/// ```
///
/// # SAFETY
///
/// - `ctxt` must be a valid pointer to a RegExecCtxt previously returned
///   by `xmlRegNewExecCtxt`, or NULL.
#[no_mangle]
pub unsafe extern "C" fn xmlRegFreeExecCtxt(ctxt: *mut RegExecCtxt) {
    if ctxt.is_null() {
        return;
    }
    unsafe {
        // SAFETY: The Vec inside RegExecCtxt was allocated by Rust's allocator
        // and must be dropped before freeing the struct memory via libc::free.
        core::ptr::drop_in_place(&mut (*ctxt).current_states);
        xmlFreeImpl(ctxt as *mut c_void);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    /// Helper: create a null-terminated xmlChar* from a byte slice using xmlMalloc.
    ///
    /// This ensures the returned pointer uses the same allocator as xmlFreeImpl,
    /// preventing allocator mismatch crashes.
    fn to_xml_str(s: &[u8]) -> *mut xmlChar {
        let len = s.len();
        let ptr =
            unsafe { xmlMallocImpl((len + 1) * core::mem::size_of::<xmlChar>()) } as *mut xmlChar;
        if ptr.is_null() {
            return ptr::null_mut();
        }
        unsafe {
            core::ptr::copy_nonoverlapping(s.as_ptr(), ptr, len);
            *ptr.add(len) = 0;
        }
        ptr
    }

    /// Helper: match a regex pattern against an input string.
    fn match_regex(pattern: &[u8], input: &[u8]) -> c_int {
        let pat = to_xml_str(pattern);
        let val = to_xml_str(input);
        unsafe {
            let compiled = xmlRegexpCompile(pat);
            if compiled.is_null() {
                return REGEXP_ERROR;
            }
            let ret = xmlRegexpExec(compiled, val);
            xmlRegFreeRegexp(compiled);
            ret
        }
    }

    /// Helper to compile a pattern and return the compiled regex.
    fn compile(pattern: &[u8]) -> *mut XmlRegexp {
        let pat = to_xml_str(pattern);
        unsafe { xmlRegexpCompile(pat) }
    }

    // ── Simple Literal Matching ───────────────────────────────────────────

    #[test]
    fn test_literal_exact() {
        assert_eq!(match_regex(b"hello", b"hello"), REGEXP_MATCH);
    }

    #[test]
    fn test_literal_no_match() {
        assert_eq!(match_regex(b"hello", b"world"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_literal_partial_prefix() {
        // Full match required - "hel" is only a prefix
        assert_eq!(match_regex(b"hello", b"hel"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_literal_empty_pattern() {
        // Empty pattern matches empty string
        assert_eq!(match_regex(b"", b""), REGEXP_MATCH);
    }

    #[test]
    fn test_literal_empty_pattern_nonempty() {
        // Empty pattern does not match non-empty string
        assert_eq!(match_regex(b"", b"a"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_literal_single_char() {
        assert_eq!(match_regex(b"a", b"a"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a", b"b"), REGEXP_NOMATCH);
    }

    // ── Alternation ──────────────────────────────────────────────────────

    #[test]
    fn test_alternation_simple() {
        assert_eq!(match_regex(b"a|b", b"a"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a|b", b"b"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a|b", b"c"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_alternation_three() {
        assert_eq!(match_regex(b"a|b|c", b"a"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a|b|c", b"b"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a|b|c", b"c"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a|b|c", b"d"), REGEXP_NOMATCH);
    }

    // ── Quantifiers ──────────────────────────────────────────────────────

    #[test]
    fn test_zero_or_more() {
        assert_eq!(match_regex(b"a*", b""), REGEXP_MATCH);
        assert_eq!(match_regex(b"a*", b"a"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a*", b"aaa"), REGEXP_MATCH);
    }

    #[test]
    fn test_one_or_more() {
        assert_eq!(match_regex(b"a+", b"a"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a+", b"aaa"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a+", b""), REGEXP_NOMATCH);
    }

    #[test]
    fn test_zero_or_one() {
        assert_eq!(match_regex(b"a?", b""), REGEXP_MATCH);
        assert_eq!(match_regex(b"a?", b"a"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a?", b"aa"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_zero_or_more_middle() {
        // a*b matches "b", "ab", "aaab"
        assert_eq!(match_regex(b"a*b", b"b"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a*b", b"ab"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a*b", b"aaab"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a*b", b"a"), REGEXP_NOMATCH);
    }

    // ── Grouping ─────────────────────────────────────────────────────────

    #[test]
    fn test_grouping() {
        assert_eq!(match_regex(b"(a)", b"a"), REGEXP_MATCH);
        assert_eq!(match_regex(b"(a)", b"b"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_grouping_with_quantifier() {
        assert_eq!(match_regex(b"(ab)+", b"ab"), REGEXP_MATCH);
        assert_eq!(match_regex(b"(ab)+", b"abab"), REGEXP_MATCH);
        assert_eq!(match_regex(b"(ab)+", b"a"), REGEXP_NOMATCH);
    }

    // ── Anchors ──────────────────────────────────────────────────────────

    #[test]
    fn test_start_anchor() {
        assert_eq!(match_regex(b"^a", b"a"), REGEXP_MATCH);
        assert_eq!(match_regex(b"^a", b"ba"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_end_anchor() {
        assert_eq!(match_regex(b"a$", b"a"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a$", b"ba"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_both_anchors() {
        assert_eq!(match_regex(b"^a$", b"a"), REGEXP_MATCH);
        assert_eq!(match_regex(b"^a$", b"ab"), REGEXP_NOMATCH);
        assert_eq!(match_regex(b"^a$", b"ba"), REGEXP_NOMATCH);
    }

    // ── Wildcard ─────────────────────────────────────────────────────────

    #[test]
    fn test_wildcard() {
        assert_eq!(match_regex(b".", b"a"), REGEXP_MATCH);
        assert_eq!(match_regex(b".", b"1"), REGEXP_MATCH);
        assert_eq!(match_regex(b"...", b"abc"), REGEXP_MATCH);
        assert_eq!(match_regex(b"...", b"ab"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_wildcard_with_literal() {
        assert_eq!(match_regex(b"a.b", b"axb"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a.b", b"azb"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a.b", b"ab"), REGEXP_NOMATCH);
    }

    // ── Escaped Characters ───────────────────────────────────────────────

    #[test]
    fn test_escaped_newline() {
        assert_eq!(match_regex(b"a\\nb", b"a\nb"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a\\nb", b"ab"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_escaped_tab() {
        assert_eq!(match_regex(b"a\\tb", b"a\tb"), REGEXP_MATCH);
    }

    #[test]
    fn test_escaped_metachar() {
        // Escaped dot matches literal dot
        assert_eq!(match_regex(b"\\.", b"."), REGEXP_MATCH);
        assert_eq!(match_regex(b"\\.", b"a"), REGEXP_NOMATCH);
    }

    // ── Character Classes ────────────────────────────────────────────────

    #[test]
    fn test_char_class_single() {
        assert_eq!(match_regex(b"[a]", b"a"), REGEXP_MATCH);
        assert_eq!(match_regex(b"[a]", b"b"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_char_class_multiple_chars() {
        assert_eq!(match_regex(b"[abc]", b"a"), REGEXP_MATCH);
        assert_eq!(match_regex(b"[abc]", b"b"), REGEXP_MATCH);
        assert_eq!(match_regex(b"[abc]", b"c"), REGEXP_MATCH);
        assert_eq!(match_regex(b"[abc]", b"d"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_char_class_range() {
        assert_eq!(match_regex(b"[a-z]", b"a"), REGEXP_MATCH);
        assert_eq!(match_regex(b"[a-z]", b"m"), REGEXP_MATCH);
        assert_eq!(match_regex(b"[a-z]", b"z"), REGEXP_MATCH);
        assert_eq!(match_regex(b"[a-z]", b"1"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_char_class_range_with_escape() {
        assert_eq!(match_regex(b"[\\d]", b"5"), REGEXP_MATCH);
        assert_eq!(match_regex(b"[\\d]", b"a"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_multiple_char_classes() {
        assert_eq!(match_regex(b"[a-z0-9]+", b"abc123"), REGEXP_MATCH);
        assert_eq!(match_regex(b"[a-z0-9]+", b"ABC"), REGEXP_NOMATCH);
    }

    // ── Predefined Classes ───────────────────────────────────────────────

    #[test]
    fn test_digit_class() {
        assert_eq!(match_regex(b"\\d", b"5"), REGEXP_MATCH);
        assert_eq!(match_regex(b"\\d", b"a"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_word_class() {
        assert_eq!(match_regex(b"\\w+", b"hello"), REGEXP_MATCH);
        assert_eq!(match_regex(b"\\w+", b"hello123"), REGEXP_MATCH);
        assert_eq!(match_regex(b"\\w+", b""), REGEXP_NOMATCH);
    }

    #[test]
    fn test_space_class() {
        assert_eq!(match_regex(b"\\s", b" "), REGEXP_MATCH);
        assert_eq!(match_regex(b"\\s", b"\t"), REGEXP_MATCH);
        assert_eq!(match_regex(b"\\s", b"a"), REGEXP_NOMATCH);
    }

    // ── Complex Patterns ─────────────────────────────────────────────────

    #[test]
    fn test_complex_email_like() {
        // Simple email-like pattern: \w+@\w+\.\w+
        assert_eq!(
            match_regex(b"\\w+@\\w+\\.\\w+", b"user@example.com"),
            REGEXP_MATCH
        );
        assert_eq!(match_regex(b"\\w+@\\w+\\.\\w+", b"invalid"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_complex_phone_like() {
        // Simple phone-like: \d{3}-\d{3}-\d{4}
        assert_eq!(
            match_regex(b"\\d{3}-\\d{3}-\\d{4}", b"555-123-4567"),
            REGEXP_MATCH
        );
        assert_eq!(
            match_regex(b"\\d{3}-\\d{3}-\\d{4}", b"555-123-456"),
            REGEXP_NOMATCH
        );
    }

    #[test]
    fn test_pattern_with_all_features() {
        // Pattern using alternation, grouping, quantifiers, anchors
        assert_eq!(match_regex(b"^(a|b)+c$", b"ac"), REGEXP_MATCH);
        assert_eq!(match_regex(b"^(a|b)+c$", b"bc"), REGEXP_MATCH);
        assert_eq!(match_regex(b"^(a|b)+c$", b"ababc"), REGEXP_MATCH);
        assert_eq!(match_regex(b"^(a|b)+c$", b"abd"), REGEXP_NOMATCH);
    }

    // ── Exact Quantifiers ────────────────────────────────────────────────

    #[test]
    fn test_exact_quantifier() {
        assert_eq!(match_regex(b"a{3}", b"aaa"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a{3}", b"aa"), REGEXP_NOMATCH);
        assert_eq!(match_regex(b"a{3}", b"aaaa"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_between_quantifier() {
        assert_eq!(match_regex(b"a{2,4}", b"aa"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a{2,4}", b"aaa"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a{2,4}", b"aaaa"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a{2,4}", b"a"), REGEXP_NOMATCH);
        assert_eq!(match_regex(b"a{2,4}", b"aaaaa"), REGEXP_NOMATCH);
    }

    #[test]
    fn test_at_least_quantifier() {
        assert_eq!(match_regex(b"a{2,}", b"aa"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a{2,}", b"aaaa"), REGEXP_MATCH);
        assert_eq!(match_regex(b"a{2,}", b"a"), REGEXP_NOMATCH);
    }

    // ── Determinism ──────────────────────────────────────────────────────

    #[test]
    fn test_deterministic_literal() {
        let compiled = compile(b"hello");
        assert!(!compiled.is_null());
        assert_eq!(unsafe { xmlRegexpIsDeterministic(compiled) }, 1);
        unsafe { xmlRegFreeRegexp(compiled) };
    }

    #[test]
    fn test_non_deterministic() {
        // Alternation is non-deterministic in NFA form
        let compiled = compile(b"a|b");
        assert!(!compiled.is_null());
        assert_eq!(unsafe { xmlRegexpIsDeterministic(compiled) }, 0);
        unsafe { xmlRegFreeRegexp(compiled) };
    }

    // ── Incremental Execution ────────────────────────────────────────────

    #[test]
    fn test_incremental_empty_input() {
        let compiled = compile(b"a*");
        assert!(!compiled.is_null());
        let ctxt = unsafe { xmlRegNewExecCtxt(compiled, ptr::null_mut()) };
        assert!(!ctxt.is_null());
        // Push empty string should not match a* (no input yet)
        let ret = unsafe { xmlRegExecPushString(ctxt, ptr::null_mut()) };
        // NULL terminates input — a* matches empty
        assert_eq!(ret, REGEXP_MATCH);
        unsafe { xmlRegFreeExecCtxt(ctxt) };
        unsafe { xmlRegFreeRegexp(compiled) };
    }

    #[test]
    fn test_incremental_simple_match() {
        let compiled = compile(b"abc");
        assert!(!compiled.is_null());
        let ctxt = unsafe { xmlRegNewExecCtxt(compiled, ptr::null_mut()) };
        assert!(!ctxt.is_null());
        let val = to_xml_str(b"abc");
        let ret = unsafe { xmlRegExecPushString(ctxt, val) };
        assert_eq!(ret, REGEXP_MATCH);
        unsafe { xmlRegFreeExecCtxt(ctxt) };
        unsafe { xmlRegFreeRegexp(compiled) };
    }

    // ── Edge Cases ───────────────────────────────────────────────────────

    #[test]
    fn test_null_pattern() {
        let compiled = unsafe { xmlRegexpCompile(ptr::null()) };
        assert!(compiled.is_null());
    }

    #[test]
    fn test_null_input() {
        let compiled = compile(b"a");
        assert!(!compiled.is_null());
        let ret = unsafe { xmlRegexpExec(compiled, ptr::null()) };
        assert_eq!(ret, REGEXP_ERROR);
        unsafe { xmlRegFreeRegexp(compiled) };
    }

    #[test]
    fn test_double_free() {
        let compiled = compile(b"test");
        assert!(!compiled.is_null());
        unsafe { xmlRegFreeRegexp(compiled) };
        // Freeing again is safe (pattern is null after free)
        // We just test that it doesn't crash
    }

    #[test]
    fn test_print() {
        let compiled = compile(b"hello");
        assert!(!compiled.is_null());
        unsafe { xmlRegexpPrint(compiled) };
        unsafe { xmlRegFreeRegexp(compiled) };
    }
}
