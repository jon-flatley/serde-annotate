use crate::color::ColorProfile;
use crate::document::{CommentFormat, Document, StrFormat};
use crate::error::Error;
use crate::integer::{Base, Int};
use once_cell::sync::OnceCell;
use std::collections::HashSet;
use std::fmt;

type Result<T> = std::result::Result<T, Error>;

/// Multiline string style to use in JSON documents.
#[derive(Clone, Copy, PartialEq)]
pub enum Multiline {
    None,
    Json5,
    Hjson,
}

/// A JSON document and its formatting properties.
pub struct Json {
    document: Document,
    indent: usize,
    color: ColorProfile,
    comment: HashSet<CommentFormat>,
    standard_comment: CommentFormat,
    bases: HashSet<Base>,
    literals: HashSet<Base>,
    strict_numeric_limits: bool,
    multiline: Multiline,
    bare_keys: bool,
    compact: bool,
}

impl Json {
    /// Set the amount of indentation for each level of nesting.
    pub fn indent(mut self, i: usize) -> Self {
        self.indent = i;
        self
    }
    /// Set the comment style to use in the document.
    pub fn comment(mut self, c: &[CommentFormat]) -> Self {
        for x in c {
            self.comment.insert(*x);
        }
        self
    }
    /// Set the comment style to use in the document.
    pub fn standard_comment(mut self, c: CommentFormat) -> Self {
        self.standard_comment = c;
        self
    }

    /// Set the allowable bases for integers.
    /// Note: an allowed base that is _not_ allowed for literals will be
    /// emitted as a quoted string.
    pub fn bases(mut self, b: &[Base]) -> Self {
        for x in b {
            self.bases.insert(*x);
        }
        self
    }
    /// Set the allowable bases for integer literals.
    /// Note: bases allowed as literals will be emitted directly into
    /// the document.
    pub fn literals(mut self, b: &[Base]) -> Self {
        for x in b {
            self.bases.insert(*x);
            self.literals.insert(*x);
        }
        self
    }
    /// Set whether to obey strict numeric limits on integer values.
    /// When true, any number larger than 2^53 in magnitude will be
    /// emitted as a quoted string.
    pub fn strict_numeric_limits(mut self, b: bool) -> Self {
        self.strict_numeric_limits = b;
        self
    }
    /// Set the style of multiline strings to be used in the document.
    pub fn multiline(mut self, m: Multiline) -> Self {
        self.multiline = m;
        self
    }
    /// Set whether bare keys in mappings are allowed.
    pub fn bare_keys(mut self, b: bool) -> Self {
        self.bare_keys = b;
        self
    }
    /// Set whether or not to use compact form.
    /// Compact form eliminates comments, newlines and indentation.
    pub fn compact(mut self, b: bool) -> Self {
        self.compact = b;
        self
    }

    pub fn color(mut self, c: ColorProfile) -> Self {
        self.color = c;
        self
    }
}

impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut emitter = JsonEmitter {
            level: 0,
            indent: self.indent,
            color: self.color,
            comment: self.comment.clone(),
            standard_comment: self.standard_comment,
            bases: self.bases.clone(),
            literals: self.literals.clone(),
            strict_numeric_limits: self.strict_numeric_limits,
            multiline: self.multiline,
            bare_keys: self.bare_keys,
            compact: self.compact,
        };
        emitter.emit_node(f, &self.document).map_err(|_| fmt::Error)
    }
}

impl Document {
    /// Convert a `Document` to a JSON document.
    pub fn to_json(self) -> Json {
        Json {
            document: self,
            indent: 2,
            color: ColorProfile::default(),
            comment: HashSet::new(),
            standard_comment: CommentFormat::SlashSlash,
            bases: HashSet::from([Base::Dec]),
            literals: HashSet::from([Base::Dec]),
            strict_numeric_limits: true,
            multiline: Multiline::None,
            bare_keys: false,
            compact: false,
        }
    }

    /// Convert a `Document` to a Json5 document.
    /// A Json5 document allows `//` comments, hex literals,
    /// multiline strings and bare keys.
    pub fn to_json5(self) -> Json {
        self.to_json()
            .comment(&[CommentFormat::Block, CommentFormat::SlashSlash])
            .literals(&[Base::Hex])
            .multiline(Multiline::Json5)
            .bare_keys(true)
    }

    /// Convert a `Document` to a Hjson document.
    /// A Hjson document allows comments, multiline strings and bare keys.
    /// Defaults to `#` comments, but hjson also supports `//` comments.
    pub fn to_hjson(self) -> Json {
        self.to_json()
            .comment(&[
                CommentFormat::Block,
                CommentFormat::Hash,
                CommentFormat::SlashSlash,
            ])
            .standard_comment(CommentFormat::Hash)
            .multiline(Multiline::Hjson)
            .bare_keys(true)
    }
}

struct JsonEmitter {
    level: usize,
    indent: usize,
    color: ColorProfile,
    comment: HashSet<CommentFormat>,
    standard_comment: CommentFormat,
    bases: HashSet<Base>,
    literals: HashSet<Base>,
    strict_numeric_limits: bool,
    multiline: Multiline,
    bare_keys: bool,
    compact: bool,
}

impl Default for JsonEmitter {
    fn default() -> Self {
        JsonEmitter {
            level: 0,
            indent: 2,
            comment: HashSet::new(),
            standard_comment: CommentFormat::SlashSlash,
            color: ColorProfile::default(),
            bases: HashSet::new(),
            literals: HashSet::new(),
            strict_numeric_limits: true,
            multiline: Multiline::None,
            bare_keys: false,
            compact: false,
        }
    }
}

impl JsonEmitter {
    fn emit_node<W: fmt::Write>(&mut self, w: &mut W, node: &Document) -> Result<()> {
        match node {
            Document::Comment(c, f) => self.emit_comment_newline(w, c, f),
            Document::String(v, f) => self.emit_string(w, v.as_str(), *f),
            Document::StaticStr(v, f) => self.emit_string(w, v, *f),
            Document::Boolean(v) => self.emit_boolean(w, *v),
            Document::Int(v) => self.emit_int(w, v),
            Document::Float(v) => self.emit_float(w, *v),
            Document::Mapping(m) => self.emit_mapping(w, m),
            Document::Sequence(s) => self.emit_sequence(w, s),
            Document::Bytes(v) => self.emit_bytes(w, v),
            Document::Null => self.emit_null(w),
            Document::Compact(d) => self.emit_compact(w, d),
            Document::Fragment(ds) => {
                match &ds[..] {
                    // Currently, an enum unit-variant is the only place in the serializer where a
                    // Fragment is constructed placing the comment after the node.  For this case,
                    // we want to emit the variant name followed by the comment on the same line.
                    [n, Document::Comment(c, f)] => {
                        self.emit_node(w, n)?;
                        if !self.compact && !self.comment.is_empty() {
                            write!(w, " ")?;
                            self.emit_comment(w, c, f)?;
                        }
                    }
                    _ => {
                        let mut prior_val = false;
                        for d in ds {
                            if prior_val {
                                self.writeln(w, "")?;
                                self.emit_indent(w)?;
                            }
                            self.emit_node(w, d)?;
                            prior_val = d.has_value();
                        }
                    }
                };
                Ok(())
            }
        }
    }

    fn emit_compact<W: fmt::Write>(&mut self, w: &mut W, node: &Document) -> Result<()> {
        let compact = self.compact;
        self.compact = true;
        self.emit_node(w, node)?;
        self.compact = compact;
        Ok(())
    }

    fn emit_bytes<W: fmt::Write>(&mut self, w: &mut W, bytes: &[u8]) -> Result<()> {
        self.level += 1;
        self.writeln(w, &self.color.aggregate.paint("[").to_string())?;
        self.emit_indent(w)?;
        for (i, value) in bytes.iter().enumerate() {
            if i > 0 {
                self.writeln(w, ",")?;
                self.emit_indent(w)?;
            }
            write!(w, "{}", value)?;
        }
        self.writeln(w, "")?;
        self.level -= 1;
        self.emit_indent(w)?;
        write!(w, "{}", &self.color.aggregate.paint("]"))?;
        Ok(())
    }

    // TODO: Can this function be rewritten to be less complex?
    fn emit_sequence<W: fmt::Write>(&mut self, w: &mut W, sequence: &[Document]) -> Result<()> {
        self.level += 1;
        self.writeln(w, &self.color.aggregate.paint("[").to_string())?;
        if !sequence.is_empty() {
            self.emit_indent(w)?;
        }
        let last = Document::last_value_index(sequence);
        let mut need_eol = false;
        for (i, value) in sequence.iter().enumerate() {
            if i > 0 && need_eol {
                write!(w, "{}", if self.compact { " " } else { "\n" })?;
                if i <= last || !self.comment.is_empty() {
                    self.emit_indent(w)?;
                }
                need_eol = false;
            }
            if let Document::Fragment(nodes) = value {
                let mut val_done = false;
                for node in nodes {
                    if let Some((c, f)) = node.comment() {
                        if val_done && need_eol {
                            write!(w, " ")?;
                        }
                        need_eol = self.emit_comment(w, c, f)?;
                        if need_eol && !val_done {
                            writeln!(w)?;
                            self.emit_indent(w)?;
                        }
                        need_eol |= i < last;
                        continue;
                    }
                    if !val_done {
                        self.emit_node(w, node)?;
                        if i != last {
                            write!(w, "{}", &self.color.punctuation.paint(","))?;
                        }
                        val_done = true;
                        need_eol = true;
                    } else {
                        return Err(Error::StructureError("Comment", node.variant()));
                    }
                }
            } else {
                self.emit_node(w, value)?;
                if i != last {
                    write!(w, "{}", &self.color.punctuation.paint(","))?;
                }
                need_eol = true;
            }
        }
        if need_eol {
            self.writeln(w, "")?;
        }
        self.level -= 1;
        self.emit_indent(w)?;
        write!(w, "{}", &self.color.aggregate.paint("]"))?;
        Ok(())
    }

    fn emit_key<W: fmt::Write>(&mut self, w: &mut W, s: &str) -> Result<()> {
        if self.bare_keys && is_legal_bareword(s) {
            write!(w, "{}", self.color.key.paint(s))?
        } else {
            write!(
                w,
                "{}{}{}",
                self.color.punctuation.paint("\""),
                self.color.key.paint(s),
                self.color.punctuation.paint("\"")
            )?
        }
        Ok(())
    }

    // TODO: Can this function be rewritten to be less complex?
    fn emit_mapping<W: fmt::Write>(&mut self, w: &mut W, mapping: &[Document]) -> Result<()> {
        self.level += 1;
        self.writeln(w, &self.color.aggregate.paint("{").to_string())?;
        if !mapping.is_empty() {
            self.emit_indent(w)?;
        }
        let last = Document::last_value_index(mapping);
        let mut need_eol = false;
        for (i, frag) in mapping.iter().enumerate() {
            let nodes = frag.fragments()?;
            if i > 0 && need_eol {
                write!(w, "{}", if self.compact { " " } else { "\n" })?;
                if i <= last || !self.comment.is_empty() {
                    self.emit_indent(w)?;
                }
                need_eol = false;
            }
            let mut key_done = i > last;
            let mut val_done = i > last;
            for node in nodes {
                if let Some((c, f)) = node.comment() {
                    if val_done && need_eol {
                        write!(w, " ")?;
                    }
                    need_eol = self.emit_comment(w, c, f)?;
                    if need_eol && !key_done {
                        writeln!(w)?;
                        self.emit_indent(w)?;
                    }
                    need_eol |= i < last;
                    continue;
                }
                if !key_done {
                    match node {
                        Document::String(s, _) => self.emit_key(w, s.as_str())?,
                        Document::StaticStr(s, _) => self.emit_key(w, s)?,
                        Document::Boolean(v) => write!(
                            w,
                            "{}{}{}",
                            self.color.punctuation.paint("\""),
                            self.color.key.paint(format!("{}", v)),
                            self.color.punctuation.paint("\"")
                        )?,
                        Document::Int(v) => write!(
                            w,
                            "{}{}{}",
                            self.color.punctuation.paint("\""),
                            self.color.key.paint(format!("{}", v)),
                            self.color.punctuation.paint("\"")
                        )?,
                        Document::Float(v) => write!(
                            w,
                            "{}{}{}",
                            self.color.punctuation.paint("\""),
                            self.color.key.paint(format!("{}", v)),
                            self.color.punctuation.paint("\"")
                        )?,
                        Document::Comment(_, _) => return Err(Error::KeyTypeError("comment")),
                        Document::Mapping(_) => return Err(Error::KeyTypeError("mapping")),
                        Document::Sequence(_) => return Err(Error::KeyTypeError("sequence")),
                        Document::Bytes(_) => return Err(Error::KeyTypeError("bytes")),
                        Document::Compact(_) => return Err(Error::KeyTypeError("compact")),
                        Document::Fragment(_) => return Err(Error::KeyTypeError("fragment")),
                        Document::Null => return Err(Error::KeyTypeError("null")),
                    };
                    write!(w, "{}", &self.color.punctuation.paint(": "))?;
                    key_done = true;
                } else if !val_done {
                    self.emit_node(w, node)?;
                    if i != last {
                        write!(w, "{}", &self.color.punctuation.paint(","))?;
                    }
                    val_done = true;
                    need_eol = true;
                }
            }
        }
        if need_eol {
            self.writeln(w, "")?;
        }
        self.level -= 1;
        self.emit_indent(w)?;
        write!(w, "{}", &self.color.aggregate.paint("}"))?;
        Ok(())
    }

    fn emit_comment_newline<W: fmt::Write>(
        &mut self,
        w: &mut W,
        comment: &str,
        format: &CommentFormat,
    ) -> Result<()> {
        if self.emit_comment(w, comment, format)? {
            writeln!(w)?;
            self.emit_indent(w)?;
        }
        Ok(())
    }

    fn emit_comment<W: fmt::Write>(
        &mut self,
        w: &mut W,
        comment: &str,
        format: &CommentFormat,
    ) -> Result<bool> {
        if self.compact || self.comment.is_empty() {
            return Ok(false);
        }
        let format = *self.comment.get(format).unwrap_or(&self.standard_comment);
        let leader = match format {
            CommentFormat::SlashSlash | CommentFormat::Standard => "//",
            CommentFormat::Hash => "#",
            CommentFormat::Block => " *",
        };
        if format == CommentFormat::Block {
            writeln!(w, "/*")?;
            self.emit_indent(w)?;
        }
        for (i, line) in comment.split('\n').enumerate() {
            if i > 0 {
                writeln!(w)?;
                self.emit_indent(w)?;
            }
            if line.is_empty() {
                write!(w, "{}", self.color.comment.paint(leader))?;
            } else {
                write!(
                    w,
                    "{}",
                    self.color.comment.paint(format!("{} {}", leader, line))
                )?;
            }
        }
        if format == CommentFormat::Block {
            writeln!(w, "*/")?;
            self.emit_indent(w)?;
        }
        Ok(true)
    }

    fn emit_string<W: fmt::Write>(&mut self, w: &mut W, value: &str, f: StrFormat) -> Result<()> {
        if self.multiline != Multiline::None && f == StrFormat::Multiline {
            self.emit_string_multiline(w, value)
        } else {
            self.emit_string_strict(w, value)
        }
    }

    fn emit_string_strict<W: fmt::Write>(&mut self, w: &mut W, value: &str) -> Result<()> {
        write!(w, "{}", &self.color.punctuation.paint("\""))?;
        let bytes = value.as_bytes();
        let mut start = 0;
        for (i, &byte) in bytes.iter().enumerate() {
            let escape = ESCAPE[byte as usize];
            if escape == 0 {
                continue;
            }
            if start < i {
                write!(w, "{}", &self.color.string.paint(&value[start..i]))?;
            }
            match escape {
                UU => write!(
                    w,
                    "{}",
                    &self.color.escape.paint(format!("\\u{:04x}", byte))
                )?,
                _ => write!(
                    w,
                    "{}",
                    &self.color.escape.paint(format!("\\{}", escape as char))
                )?,
            };
            start = i + 1;
        }
        if start != bytes.len() {
            write!(w, "{}", &self.color.string.paint(&value[start..]))?;
        }
        write!(w, "{}", &self.color.punctuation.paint("\""))?;
        Ok(())
    }

    fn emit_string_multiline<W: fmt::Write>(&mut self, w: &mut W, value: &str) -> Result<()> {
        if self.multiline == Multiline::Hjson {
            writeln!(w)?;
            self.level += 1;
            self.emit_indent(w)?;
            self.writeln(w, &self.color.punctuation.paint("'''").to_string())?;
            self.emit_indent(w)?;
        } else {
            write!(w, "{}", &self.color.punctuation.paint("\""))?;
        }
        let bytes = value.as_bytes();
        let mut start = 0;
        for (i, &byte) in bytes.iter().enumerate() {
            let escape = ESCAPE[byte as usize];
            if escape == 0 {
                continue;
            }
            if start < i {
                write!(w, "{}", &self.color.string.paint(&value[start..i]))?;
            }
            match escape {
                UU => write!(
                    w,
                    "{}",
                    &self.color.escape.paint(format!("\\u{:04x}", byte))
                )?,
                NN => match self.multiline {
                    Multiline::None => write!(
                        w,
                        "{}",
                        &self.color.escape.paint(format!("\\{}", escape as char))
                    )?,
                    Multiline::Json5 => writeln!(w, "{}", self.color.escape.paint("\\"))?,
                    Multiline::Hjson => {
                        writeln!(w)?;
                        self.emit_indent(w)?;
                    }
                },
                _ => write!(
                    w,
                    "{}",
                    &self.color.escape.paint(format!("\\{}", escape as char))
                )?,
            };
            start = i + 1;
        }
        if start != bytes.len() {
            write!(w, "{}", &self.color.string.paint(&value[start..]))?;
        }
        if self.multiline == Multiline::Hjson {
            writeln!(w)?;
            self.emit_indent(w)?;
            write!(w, "{}", &self.color.punctuation.paint("'''"))?;
            self.level -= 1;
        } else {
            write!(w, "{}", &self.color.punctuation.paint("\""))?;
        }
        Ok(())
    }

    fn emit_boolean<W: fmt::Write>(&mut self, w: &mut W, b: bool) -> Result<()> {
        if b {
            write!(w, "{}", &self.color.boolean.paint("true"))?;
        } else {
            write!(w, "{}", &self.color.boolean.paint("false"))?;
        }
        Ok(())
    }

    fn emit_int<W: fmt::Write>(&mut self, w: &mut W, i: &Int) -> Result<()> {
        let b = i.base();
        let s = i.format(self.bases.get(&b));
        if self.strict_numeric_limits && !i.is_legal_json()
            || self.bases.get(&b).is_some() && self.literals.get(&b).is_none()
        {
            write!(
                w,
                "{}{}{}",
                self.color.punctuation.paint("\""),
                self.color.integer.paint(s),
                self.color.punctuation.paint("\"")
            )?;
        } else {
            write!(w, "{}", &self.color.integer.paint(s))?;
        }
        Ok(())
    }

    fn emit_float<W: fmt::Write>(&mut self, w: &mut W, f: f64) -> Result<()> {
        write!(w, "{}", &self.color.float.paint(format!("{}", f)))?;
        Ok(())
    }

    fn emit_null<W: fmt::Write>(&mut self, w: &mut W) -> Result<()> {
        write!(w, "{}", &self.color.null.paint("null"))?;
        Ok(())
    }

    fn emit_indent<W: fmt::Write>(&mut self, w: &mut W) -> Result<()> {
        if self.compact {
            return Ok(());
        }
        let mut len = self.level * self.indent;
        while len > 0 {
            let chunk = std::cmp::min(len, SPACE.len());
            write!(w, "{}", &SPACE[..chunk])?;
            len -= chunk;
        }
        Ok(())
    }

    fn writeln<W: fmt::Write>(&mut self, w: &mut W, s: &str) -> Result<()> {
        if self.compact {
            match s {
                "," => write!(w, "{} ", self.color.punctuation.paint(","))?,
                _ => write!(w, "{}", s)?,
            };
        } else {
            match s {
                "," => writeln!(w, "{}", self.color.punctuation.paint(","))?,
                _ => writeln!(w, "{}", s)?,
            };
        }
        Ok(())
    }
}

// Taken from serde-json:
const BB: u8 = b'b'; // \x08
const TT: u8 = b't'; // \x09
const NN: u8 = b'n'; // \x0A
const FF: u8 = b'f'; // \x0C
const RR: u8 = b'r'; // \x0D
const QU: u8 = b'"'; // \x22
const BS: u8 = b'\\'; // \x5C
const UU: u8 = b'u'; // \x00...\x1F except the ones above
const __: u8 = 0;

// Lookup table of escape sequences. A value of b'x' at index i means that byte
// i is escaped as "\x" in JSON. A value of 0 means that byte i is not escaped.
const ESCAPE: [u8; 256] = [
    //   1   2   3   4   5   6   7   8   9   A   B   C   D   E   F
    UU, UU, UU, UU, UU, UU, UU, UU, BB, TT, NN, UU, FF, RR, UU, UU, // 0
    UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, UU, // 1
    __, __, QU, __, __, __, __, __, __, __, __, __, __, __, __, __, // 2
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 3
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 4
    __, __, __, __, __, __, __, __, __, __, __, __, BS, __, __, __, // 5
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 6
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 7
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 8
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // 9
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // A
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // B
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // C
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // D
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // E
    __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, __, // F
];

const SPACE: &str = "                                                                                                    ";

// More strict than javascript.
fn bad_identifier_char(ch: char) -> bool {
    match ch {
        '0'..='9' => false,
        'A'..='Z' => false,
        'a'..='z' => false,
        '_' => false,
        '$' => false,
        _ => true,
    }
}

fn is_reserved_word(word: &str) -> bool {
    static WORDS: OnceCell<HashSet<&str>> = OnceCell::new();
    let words = WORDS.get_or_init(|| {
        HashSet::from([
            "break",
            "do",
            "instanceof",
            "typeof",
            "case",
            "else",
            "new",
            "var",
            "catch",
            "finally",
            "return",
            "void",
            "continue",
            "for",
            "switch",
            "while",
            "debugger",
            "function",
            "this",
            "with",
            "default",
            "if",
            "throw",
            "",
            "delete",
            "in",
            "try",
            "class",
            "enum",
            "extends",
            "super",
            "const",
            "export",
            "import",
            "implements",
            "let",
            "private",
            "public",
            "yield",
            "interface",
            "package",
            "protected",
            "static",
            "null",
            "true",
            "false",
        ])
    });
    words.get(word).is_some()
}

fn is_legal_bareword(word: &str) -> bool {
    if word.len() == 0 {
        return false;
    }
    let ch = word.chars().nth(0).unwrap();
    !((ch >= '0' && ch <= '9') || word.contains(bad_identifier_char) || is_reserved_word(word))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::CommentFormat;

    fn int(v: i32) -> Document {
        Document::Int(Int::new(v, Base::Dec))
    }
    fn hex(v: u32) -> Document {
        Document::Int(Int::new(v, Base::Hex))
    }
    fn float(v: f64) -> Document {
        Document::Float(v)
    }
    fn boolean(v: bool) -> Document {
        Document::Boolean(v)
    }
    fn null() -> Document {
        Document::Null
    }
    fn string(v: &str) -> Document {
        Document::String(v.to_string(), StrFormat::Standard)
    }
    fn multistr(v: &str) -> Document {
        Document::String(v.to_string(), StrFormat::Multiline)
    }
    fn comment(v: &str) -> Document {
        Document::Comment(v.to_string(), CommentFormat::Standard)
    }
    fn kv(k: &str, v: Document) -> Document {
        Document::Fragment(vec![string(k), v])
    }
    fn kvcomment(k: &str, v: Document, c: &str) -> Document {
        Document::Fragment(vec![comment(c), string(k), v])
    }
    fn nes_address(seg: &str, bank: i32, addr: u32) -> Document {
        Document::Compact(
            Document::Mapping(vec![kv(
                seg,
                Document::Sequence(vec![int(bank), hex(addr)]),
            )])
            .into(),
        )
    }

    #[test]
    fn basic_document() {
        let c = comment("woohoo!").to_json();
        assert_eq!(c.to_string(), "");
        let c = comment("woohoo!").to_json5();
        assert_eq!(c.to_string(), "// woohoo!\n");
        let n = null().to_json();
        assert_eq!(n.to_string(), "null");
        let b = boolean(true).to_json();
        assert_eq!(b.to_string(), "true");
        // Plain integer
        let i = int(5).to_json();
        assert_eq!(i.to_string(), "5");
        // Integer wants to be hex, but hex isn't allowed.
        let i = hex(15).to_json();
        assert_eq!(i.to_string(), "15");
        // Integer wants to be hex, hex is allowed, but not as a literal.
        let i = hex(16).to_json().bases(&[Base::Hex]);
        assert_eq!(i.to_string(), "\"0x10\"");
        // Integer wants to be hex, hex literals allowed.
        let i = hex(16).to_json5();
        assert_eq!(i.to_string(), "0x10");
        let s = string("hello").to_json();
        assert_eq!(s.to_string(), "\"hello\"");
        let f = float(3.14159).to_json();
        assert_eq!(f.to_string(), "3.14159");
    }

    #[test]
    fn basic_list() {
        let expect = r#"[
  5,
  10,
  15,
  "foo"
]"#;

        let list = Document::Sequence(vec![int(5), int(10), int(15), string("foo")]).to_json();
        assert_eq!(list.to_string(), expect);
    }

    #[test]
    fn basic_map() {
        let expect = r#"{
  "a": 5,
  "b": 10,
  "c": 15,
  "true": "foo"
}"#;
        let map = Document::Mapping(vec![
            kv("a", int(5)),
            kv("b", int(10)),
            kv("c", int(15)),
            kv("true", string("foo")),
        ])
        .to_json();
        assert_eq!(map.to_string(), expect);
    }

    #[test]
    fn basic_map5() {
        let expect = r#"{
  a: 5,
  b: 10,
  c: 0xF,
  "true": "foo"
}"#;
        let map = Document::Mapping(vec![
            kv("a", int(5)),
            kv("b", int(10)),
            kv("c", hex(15)),
            kv("true", string("foo")),
        ])
        .to_json5();
        assert_eq!(map.to_string(), expect);
    }

    #[test]
    fn compact_map5() {
        let expect = r#"{a: 5, b: 10, c: 0xF, "true": "foo"}"#;
        let map = Document::Mapping(vec![
            kv("a", int(5)),
            kv("b", int(10)),
            kv("c", hex(15)),
            kv("true", string("foo")),
        ])
        .to_json5()
        .compact(true);
        assert_eq!(map.to_string(), expect);
    }

    #[test]
    fn mixed_map5() {
        let expect = r#"{
  gameplay: {prg: [0, 0x8000]},
  overworld: {prg: [1, 0x8000]},
  palaces: {prg: [4, 0x8000]},
  title: {prg: [5, 0x8000]},
  music: {prg: [6, 0x8000]},
  reset: {prg: [-1, 0xFFFA]}
}"#;
        let map = Document::Mapping(vec![
            kv("gameplay", nes_address("prg", 0, 0x8000)),
            kv("overworld", nes_address("prg", 1, 0x8000)),
            kv("palaces", nes_address("prg", 4, 0x8000)),
            kv("title", nes_address("prg", 5, 0x8000)),
            kv("music", nes_address("prg", 6, 0x8000)),
            kv("reset", nes_address("prg", -1, 0xFFFA)),
        ])
        .to_json5();
        assert_eq!(map.to_string(), expect);
    }

    #[test]
    fn demo_map5() {
        let expect = r#"{
  // comments
  unquoted: "and you can quote me on that",
  singleQuotes: "not really, though",
  lineBreaks: "Look, Mom! \
No \\n's!",
  hexadecimal: 0xDECAF,
  "leadingDecimal(not)": 0.8675309,
  "andTrailing(not)": 8675309,
  "positiveSign(not)": 1,
  "trailingComma(not)": [
    "in objects",
    "or arrays"
  ],
  backwardsCompatible: "with JSON"
}"#;
        let map = Document::Mapping(vec![
            kvcomment(
                "unquoted",
                string("and you can quote me on that"),
                "comments",
            ),
            kv("singleQuotes", string("not really, though")),
            kv("lineBreaks", multistr("Look, Mom! \nNo \\n's!")),
            kv("hexadecimal", hex(0xdecaf)),
            kv("leadingDecimal(not)", float(0.8675309)),
            kv("andTrailing(not)", float(8675309.0)),
            kv("positiveSign(not)", int(1)),
            kv(
                "trailingComma(not)",
                Document::Sequence(vec![string("in objects"), string("or arrays")]),
            ),
            kv("backwardsCompatible", string("with JSON")),
        ])
        .to_json5();
        assert_eq!(map.to_string(), expect);
    }

    #[test]
    fn demo_maph() {
        let expect = r#"{
  # comments
  unquoted: "and you can quote me on that",
  singleQuotes: "not really, though",
  lineBreaks: 
    '''
    Look, Mom!
    No \\n's!
    ''',
  hexadecimal: 912559,
  "leadingDecimal(not)": 0.8675309,
  "andTrailing(not)": 8675309,
  "positiveSign(not)": 1,
  "trailingComma(not)": [
    "in objects",
    "or arrays"
  ],
  backwardsCompatible: "with JSON"
}"#;
        let map = Document::Mapping(vec![
            kvcomment(
                "unquoted",
                string("and you can quote me on that"),
                "comments",
            ),
            kv("singleQuotes", string("not really, though")),
            kv("lineBreaks", multistr("Look, Mom!\nNo \\n's!")),
            kv("hexadecimal", hex(0xdecaf)),
            kv("leadingDecimal(not)", float(0.8675309)),
            kv("andTrailing(not)", float(8675309.0)),
            kv("positiveSign(not)", int(1)),
            kv(
                "trailingComma(not)",
                Document::Sequence(vec![string("in objects"), string("or arrays")]),
            ),
            kv("backwardsCompatible", string("with JSON")),
        ])
        .to_hjson();
        println!("{}", map);
        assert_eq!(map.to_string(), expect);
    }
}
