//! A tolerant parser for a ZMK `.keymap`: the devicetree subset keymaps
//! use (nodes, labels, `&label { … }` overrides, string / cell / byte
//! properties) after a light preprocessor pass (comments, line
//! continuations, object-like `#define`s; `#include` and conditionals are
//! skipped).
//!
//! It extracts what `keymap diff` needs: each layer of the
//! `compatible = "zmk,keymap"` node in order (name from `display-name`,
//! else the node name) with its `bindings` split into `&label params…` and
//! the parameters evaluated to the values the firmware stores; and every
//! labelled behaviour node's names (node name, `label`, `display-name`), so
//! the firmware's display names map back to labels. What it can't make
//! sense of becomes a note in [`KeymapFile::notes`], never a failure, except
//! a file with no keymap node at all.

use std::collections::{BTreeSet, HashMap};

use super::expr::{self, Defines};

/// A parsed `.keymap`.
#[derive(Debug, Clone, Default)]
pub struct KeymapFile {
    pub layers: Vec<FileLayer>,
    /// Display-name candidates (node name, `label`, `display-name`) of the
    /// file's behaviour nodes → their devicetree label.
    pub behavior_names: HashMap<String, String>,
    /// The file's object-like `#define`s.
    pub defines: Defines,
    /// `ZMK_POINTING_DEFAULT_MOVE_VAL` / `_SCRL_VAL` as the file sets them.
    pub move_val: i32,
    pub scrl_val: i32,
    /// What was skipped or couldn't be evaluated, for the user.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileLayer {
    pub index: usize,
    /// The devicetree node name (`layer_1`).
    pub node: String,
    /// `display-name`, else the node name.
    pub name: String,
    pub bindings: Vec<FileBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileBinding {
    pub position: usize,
    pub label: String,
    pub params: Vec<FileParam>,
    /// The cells as written (after `#define` expansion), for display when a
    /// parameter couldn't be evaluated.
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileParam {
    Value(u32),
    /// Couldn't be evaluated (an unknown name, a function-like macro).
    Unresolved(String),
}

impl FileBinding {
    pub fn value(&self, i: usize) -> u32 {
        match self.params.get(i) {
            Some(FileParam::Value(v)) => *v,
            _ => 0,
        }
    }

    pub fn resolved(&self) -> bool {
        self.params.iter().all(|p| matches!(p, FileParam::Value(_)))
    }
}

/// Parses a `.keymap`. `Err` only when there's no `zmk,keymap` node.
pub fn parse(src: &str) -> Result<KeymapFile, String> {
    let mut notes = Vec::new();
    let pre = preprocess(src, &mut notes);
    let toks = tokenize(&pre.text, &mut notes);
    let mut p = Parser {
        toks: &toks,
        i: 0,
        notes: &mut notes,
    };
    let roots = p.items();
    let defines = Defines { file: pre.defines };
    let eval_int =
        |name: &str, default: i32| expr::eval(name, &defines).map_or(default, |v| v as i32);
    let mut file = KeymapFile {
        move_val: eval_int("ZMK_POINTING_DEFAULT_MOVE_VAL", super::DEFAULT_MOVE_VAL),
        scrl_val: eval_int("ZMK_POINTING_DEFAULT_SCRL_VAL", super::DEFAULT_SCRL_VAL),
        defines,
        ..Default::default()
    };
    let mut keymap = None;
    walk(&roots, &mut |n| {
        if n.prop_str("compatible") == Some("zmk,keymap") && keymap.is_none() {
            keymap = Some(n.clone());
        }
        let is_behavior = n.prop("#binding-cells").is_some()
            || n.prop_str("compatible")
                .is_some_and(|c| c.starts_with("zmk,behavior"));
        if let (true, Some(label)) = (is_behavior, n.labels.first()) {
            let names = [
                Some(n.name.as_str()),
                n.prop_str("label"),
                n.prop_str("display-name"),
            ];
            for name in names.into_iter().flatten() {
                file.behavior_names
                    .entry(name.to_string())
                    .or_insert_with(|| label.clone());
            }
            file.behavior_names
                .entry(label.clone())
                .or_insert_with(|| label.clone());
        }
    });
    let keymap = keymap.ok_or("no `compatible = \"zmk,keymap\"` node found")?;
    for (index, layer) in keymap.children.iter().enumerate() {
        let name = layer
            .prop_str("display-name")
            .or_else(|| layer.prop_str("label"))
            .unwrap_or(&layer.name)
            .to_string();
        let cells: Vec<&str> = layer
            .prop("bindings")
            .map(|vals| {
                vals.iter()
                    .filter_map(|v| match v {
                        Value::Cells(c) => Some(c.as_str()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        if cells.is_empty() {
            notes.push(format!("layer {name}: no `bindings = <…>`"));
        }
        let bindings = split_bindings(&cells.join(" "), &file.defines, &pre.function_macros)
            .into_iter()
            .enumerate()
            .map(|(position, (label, items))| {
                let params = items
                    .iter()
                    .map(|it| match expr::eval(it, &file.defines) {
                        Ok(v) => FileParam::Value(v),
                        Err(e) => {
                            notes.push(format!("layer {name}, position {position}: {e}"));
                            FileParam::Unresolved(it.clone())
                        }
                    })
                    .collect();
                let mut source = format!("&{label}");
                for it in &items {
                    source.push(' ');
                    source.push_str(it);
                }
                FileBinding {
                    position,
                    label,
                    params,
                    source,
                }
            })
            .collect();
        file.layers.push(FileLayer {
            index,
            node: layer.name.clone(),
            name,
            bindings,
        });
    }
    file.notes = notes;
    Ok(file)
}

// ---- preprocessor -------------------------------------------------------

struct Pre {
    text: String,
    defines: HashMap<String, String>,
    function_macros: BTreeSet<String>,
}

const DIRECTIVES: [&str; 13] = [
    "define", "undef", "include", "if", "ifdef", "ifndef", "elif", "else", "endif", "pragma",
    "error", "warning", "line",
];

/// Strips comments, joins `\` continuations, and takes out `#` lines,
/// keeping object-like `#define`s.
fn preprocess(src: &str, notes: &mut Vec<String>) -> Pre {
    let mut text = String::with_capacity(src.len());
    let mut defines = HashMap::new();
    let mut function_macros = BTreeSet::new();
    let mut conditionals = false;
    let joined = src.replace("\\\r\n", " ").replace("\\\n", " ");
    for line in strip_comments(&joined).lines() {
        let t = line.trim_start();
        let Some(directive) = t.strip_prefix('#') else {
            text.push_str(line);
            text.push('\n');
            continue;
        };
        let directive = directive.trim_start();
        let (word, rest) = directive
            .split_once(|c: char| c.is_whitespace())
            .unwrap_or((directive, ""));
        // `#binding-cells = <2>;` is a property, not a directive.
        if !DIRECTIVES.contains(&word) {
            text.push_str(line);
            text.push('\n');
            continue;
        }
        text.push('\n');
        match word {
            "define" => {
                let rest = rest.trim_start();
                let end = rest
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(rest.len());
                let (name, body) = rest.split_at(end);
                if name.is_empty() {
                    continue;
                }
                if body.starts_with('(') {
                    function_macros.insert(name.to_string());
                } else {
                    defines.insert(name.to_string(), body.trim().to_string());
                }
            }
            "undef" => {
                defines.remove(rest.trim());
            }
            "if" | "ifdef" | "ifndef" | "elif" | "else" | "endif" => conditionals = true,
            _ => {}
        }
    }
    if conditionals {
        notes.push("#if/#ifdef blocks are not evaluated: every branch was read".into());
    }
    Pre {
        text,
        defines,
        function_macros,
    }
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    let mut in_str = false;
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                in_str = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut prev = ' ';
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                    }
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}

// ---- devicetree ---------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    /// A name: node, property, label (with the `:` split off), `&ref`, `/`.
    Word(String),
    Str(String),
    /// The text between `<` and `>`.
    Cells(String),
    /// The text between `[` and `]`.
    Bytes(String),
    Punct(char),
}

fn tokenize(s: &str, notes: &mut Vec<String>) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut it = s.char_indices().peekable();
    let word_char = |c: char| {
        c.is_ascii_alphanumeric()
            || matches!(c, '_' | ',' | '.' | '+' | '-' | '#' | '@' | '?' | '&' | '/')
    };
    while let Some(&(i, c)) = it.peek() {
        match c {
            c if c.is_whitespace() => {
                it.next();
            }
            '{' | '}' | ';' | '=' | ':' => {
                it.next();
                out.push(Tok::Punct(c));
            }
            '"' => {
                it.next();
                let mut v = String::new();
                while let Some((_, c)) = it.next() {
                    match c {
                        '\\' => {
                            if let Some((_, n)) = it.next() {
                                v.push(n);
                            }
                        }
                        '"' => break,
                        c => v.push(c),
                    }
                }
                out.push(Tok::Str(v));
            }
            '<' | '[' => {
                it.next();
                let close = if c == '<' { '>' } else { ']' };
                let mut depth = 0i32;
                let mut v = String::new();
                let mut closed = false;
                for (_, c) in it.by_ref() {
                    match c {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        c if c == close && depth <= 0 => {
                            closed = true;
                            break;
                        }
                        _ => {}
                    }
                    v.push(c);
                }
                if !closed {
                    notes.push(format!("unclosed `{c}` at byte {i}"));
                }
                out.push(if c == '<' {
                    Tok::Cells(v)
                } else {
                    Tok::Bytes(v)
                });
            }
            ',' => {
                it.next();
                out.push(Tok::Punct(','));
            }
            c if word_char(c) => {
                let mut v = String::new();
                while let Some(&(_, c)) = it.peek() {
                    if !word_char(c) || (c == ',' && v.is_empty()) {
                        break;
                    }
                    v.push(c);
                    it.next();
                }
                // `prop = <1>, <2>`: a trailing comma belongs to the list.
                let trailing = v.ends_with(',');
                let w = v.trim_end_matches(',').to_string();
                out.push(Tok::Word(w));
                if trailing {
                    out.push(Tok::Punct(','));
                }
            }
            _ => {
                it.next();
                notes.push(format!("skipped `{c}` at byte {i}"));
            }
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Str(String),
    Cells(String),
    Bytes(String),
    Ref(String),
}

#[derive(Debug, Clone, Default)]
struct Node {
    name: String,
    labels: Vec<String>,
    props: Vec<(String, Vec<Value>)>,
    children: Vec<Node>,
}

impl Node {
    fn prop(&self, name: &str) -> Option<&[Value]> {
        // The last assignment wins, as in devicetree.
        self.props
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_slice())
    }

    fn prop_str(&self, name: &str) -> Option<&str> {
        match self.prop(name)?.first()? {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }
}

fn walk<'a>(nodes: &'a [Node], f: &mut impl FnMut(&'a Node)) {
    for n in nodes {
        f(n);
        walk(&n.children, f);
    }
}

struct Parser<'a> {
    toks: &'a [Tok],
    i: usize,
    notes: &'a mut Vec<String>,
}

impl Parser<'_> {
    fn peek(&self, k: usize) -> Option<&Tok> {
        self.toks.get(self.i + k)
    }

    fn punct(&self, k: usize, c: char) -> bool {
        self.peek(k) == Some(&Tok::Punct(c))
    }

    /// Top-level items and node bodies: nodes and properties until `}` or
    /// the end.
    fn items(&mut self) -> Vec<Node> {
        let mut root = Node::default();
        loop {
            self.body(&mut root);
            if self.peek(0).is_none() {
                break;
            }
            self.notes.push("skipped an unbalanced `}`".into());
            self.i += 1;
        }
        // Top-level properties (rare) are dropped; nodes are what matter.
        root.children
    }

    fn body(&mut self, node: &mut Node) {
        while let Some(t) = self.peek(0).cloned() {
            match t {
                Tok::Punct('}') => return,
                Tok::Punct(';') => self.i += 1,
                Tok::Word(w) => {
                    let mut labels = Vec::new();
                    let mut w = w;
                    // `label: label2: name {`
                    while self.punct(1, ':') {
                        labels.push(w.clone());
                        self.i += 2;
                        match self.peek(0) {
                            Some(Tok::Word(n)) => w = n.clone(),
                            _ => break,
                        }
                    }
                    if self.punct(1, '{') {
                        self.i += 2;
                        let mut child = Node {
                            name: w.split('@').next().unwrap_or(&w).to_string(),
                            labels,
                            ..Default::default()
                        };
                        // `&label { … }` overrides an existing node: the
                        // reference is its only label.
                        if let Some(r) = w.strip_prefix('&') {
                            child.labels.push(r.to_string());
                        }
                        self.body(&mut child);
                        if self.punct(0, '}') {
                            self.i += 1;
                        } else {
                            self.notes.push(format!("node `{w}` is not closed"));
                        }
                        node.children.push(child);
                    } else if w.starts_with('/') && w.ends_with('/') && w.len() > 1 {
                        // `/dts-v1/;`, `/delete-node/ &x;`
                        self.skip_statement();
                    } else {
                        self.property(w, node);
                    }
                }
                other => {
                    self.notes.push(format!("skipped unexpected {other:?}"));
                    self.skip_statement();
                }
            }
        }
    }

    fn property(&mut self, name: String, node: &mut Node) {
        self.i += 1;
        let mut values = Vec::new();
        if self.punct(0, '=') {
            self.i += 1;
            loop {
                match self.peek(0).cloned() {
                    Some(Tok::Str(s)) => values.push(Value::Str(s)),
                    Some(Tok::Cells(c)) => values.push(Value::Cells(c)),
                    Some(Tok::Bytes(b)) => values.push(Value::Bytes(b)),
                    Some(Tok::Word(w)) if w.starts_with('&') => values.push(Value::Ref(w)),
                    _ => {
                        self.notes
                            .push(format!("property `{name}`: unexpected value"));
                        self.skip_statement();
                        return;
                    }
                }
                self.i += 1;
                if self.punct(0, ',') {
                    self.i += 1;
                } else {
                    break;
                }
            }
        }
        if self.punct(0, ';') {
            self.i += 1;
        } else {
            self.notes.push(format!("property `{name}`: missing `;`"));
            self.skip_statement();
        }
        node.props.push((name, values));
    }

    /// Skips to just after the next `;` at this nesting level (or to the
    /// `}` that closes it).
    fn skip_statement(&mut self) {
        let mut depth = 0;
        while let Some(t) = self.peek(0) {
            match t {
                Tok::Punct('{') => depth += 1,
                Tok::Punct('}') if depth == 0 => return,
                Tok::Punct('}') => depth -= 1,
                Tok::Punct(';') if depth == 0 => {
                    self.i += 1;
                    return;
                }
                _ => {}
            }
            self.i += 1;
        }
    }
}

// ---- bindings -----------------------------------------------------------

/// Splits a layer's cells into `(label, param expressions)`, expanding
/// object-like `#define`s (a define may supply several cells, or a whole
/// `&kp A`). `RGB_COLOR_HSB(h,s,v)` becomes its two cells, as ZMK's
/// token-pasting macro does.
fn split_bindings(
    cells: &str,
    defines: &Defines,
    function_macros: &BTreeSet<String>,
) -> Vec<(String, Vec<String>)> {
    let mut items = Vec::new();
    for it in cell_items(cells) {
        expand(&it, defines, 0, &mut items);
    }
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for it in items {
        if let Some(label) = it.strip_prefix('&') {
            out.push((label.to_string(), Vec::new()));
        } else if let Some((_, params)) = out.last_mut() {
            let name = it.split('(').next().unwrap_or(&it);
            if function_macros.contains(name) {
                // Left as is: evaluating it reports the unknown macro.
                params.push(it);
                continue;
            }
            match it
                .strip_prefix("RGB_COLOR_HSB(")
                .or_else(|| it.strip_prefix("RGB_COLOR_HSV("))
            {
                Some(args) => {
                    params.push("RGB_COLOR_HSB_CMD".into());
                    params.push(format!("RGB_COLOR_HSB_VAL({args}"));
                }
                None => params.push(it),
            }
        }
        // Cells before the first `&` belong to no binding: dropped.
    }
    out
}

/// One cell list as items: whitespace-separated, but parentheses keep an
/// expression together (`(1 + 2)`, `LS(A)`).
fn cell_items(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c.is_whitespace() && depth <= 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Expands an item that is a bare `#define` name (or `&NAME` whose `NAME`
/// is a define) into the define's items, recursively.
fn expand(item: &str, defines: &Defines, depth: usize, out: &mut Vec<String>) {
    let (amp, name) = match item.strip_prefix('&') {
        Some(n) => ("&", n),
        None => ("", item),
    };
    // Only the file's defines expand a `&NAME`; the built-in ones are
    // parameter names. Multi-cell built-ins (`BT_CLR`) do expand.
    let body = if amp.is_empty() {
        defines.get(name)
    } else {
        defines.file.get(name).map(String::as_str)
    };
    match body {
        Some(body) if depth < 16 && super::is_identifier(name) => {
            let mut parts = cell_items(body);
            if parts.len() == 1 && amp.is_empty() && !parts[0].starts_with('&') {
                // A single expression: keep the name, so the evaluator
                // expands it (and its error names it).
                out.push(item.to_string());
                return;
            }
            if let Some(first) = parts.first_mut() {
                first.insert_str(0, amp);
            }
            for p in parts {
                expand(&p, defines, depth + 1, out);
            }
        }
        _ => out.push(item.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::keys;

    const MINI: &str = r#"
#define MOVE 1200 // overrides ZMK's 600
#define ZMK_POINTING_DEFAULT_MOVE_VAL MOVE
#define HYPER LC(LS(LA(LGUI)))
#define KP_A &kp A
#define TWICE(x) x x
#include <behaviors.dtsi>

/* A block comment with a { brace } and a "string". */
&soft_off { hold-time-ms = <2000>; };

/ {
    behaviors {
        hml: home_row_mod_left {
            compatible = "zmk,behavior-hold-tap";
            #binding-cells = <2>;
            bindings = <&kp>, <&kp>;
        };
    };
    macros {
        M_PIPE: M_PIPE {
            compatible = "zmk,behavior-macro";
            #binding-cells = <0>;
            bindings = <&kp PIPE &kp GT>;
            label = "PIPE_OP";
        };
    };
    keymap {
        compatible = "zmk,keymap";
        base {
            display-name = "BASE";
            bindings = <
                &kp A &hml LALT S &kp LS(N1) &mo 1
                &kp HYPER KP_A &bt BT_SEL 0 &bt BT_CLR
            >;
        };
        layer_1 {
            bindings = <&trans &mmv MOVE_UP>, <&rgb_ug RGB_COLOR_HSB(120,100,50) &kp NOPE &kp TWICE(A)>;
        };
    };
};
"#;

    #[test]
    fn parses_layers_defines_and_behaviours() {
        let f = parse(MINI).unwrap();
        assert_eq!(f.layers.len(), 2);
        let base = &f.layers[0];
        assert_eq!((base.name.as_str(), base.node.as_str()), ("BASE", "base"));
        let zmk: Vec<_> = base.bindings.iter().map(|b| b.source.as_str()).collect();
        assert_eq!(
            zmk,
            [
                "&kp A",
                "&hml LALT S",
                "&kp LS(N1)",
                "&mo 1",
                "&kp HYPER",
                "&kp A",
                "&bt BT_SEL 0",
                "&bt BT_CLR_CMD 0",
            ]
        );
        let v = |b: &FileBinding| -> Vec<u32> { (0..b.params.len()).map(|i| b.value(i)).collect() };
        assert_eq!(
            v(&base.bindings[1]),
            [keys::value("LALT").unwrap(), keys::value("S").unwrap()]
        );
        assert_eq!(v(&base.bindings[2]), [keys::value("EXCL").unwrap()]);
        assert_eq!(v(&base.bindings[4]), [0x0707_00e3]);
        assert_eq!(v(&base.bindings[6]), [3, 0]);
        assert_eq!(v(&base.bindings[7]), [0, 0]);
        assert_eq!(f.move_val, 1200);
        assert_eq!(f.scrl_val, 10);
        assert_eq!(
            f.behavior_names
                .get("home_row_mod_left")
                .map(String::as_str),
            Some("hml")
        );
        assert_eq!(
            f.behavior_names.get("PIPE_OP").map(String::as_str),
            Some("M_PIPE")
        );
        assert_eq!(
            f.behavior_names.get("M_PIPE").map(String::as_str),
            Some("M_PIPE")
        );
    }

    #[test]
    fn odd_cells_become_notes() {
        let f = parse(MINI).unwrap();
        let l1 = &f.layers[1];
        assert_eq!(l1.name, "layer_1");
        let b = &l1.bindings;
        assert_eq!(b[1].source, "&mmv MOVE_UP");
        assert_eq!(b[1].value(0), 0xfb50);
        assert_eq!(
            b[2].source,
            "&rgb_ug RGB_COLOR_HSB_CMD RGB_COLOR_HSB_VAL(120,100,50)"
        );
        assert_eq!((b[2].value(0), b[2].value(1)), (14, 0x0078_6432));
        assert_eq!(b[3].params, [FileParam::Unresolved("NOPE".into())]);
        assert!(!b[3].resolved());
        assert_eq!(b[4].params, [FileParam::Unresolved("TWICE(A)".into())]);
        let notes = f.notes.join("\n");
        assert!(
            notes.contains("layer layer_1, position 3: unknown name `NOPE`"),
            "{notes}"
        );
        assert!(notes.contains("unknown macro `TWICE()`"), "{notes}");
    }

    #[test]
    fn no_keymap_node_is_an_error_and_garbage_is_tolerated() {
        assert!(parse("/ { behaviors { }; };").is_err());
        assert!(parse("").is_err());
        let f =
            parse("}}} / { keymap { compatible = \"zmk,keymap\"; l { bindings = <&kp A>; }; }; ")
                .unwrap();
        assert_eq!(f.layers.len(), 1);
        assert_eq!(f.layers[0].bindings[0].source, "&kp A");
    }

    const REAL: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../config/eyelash_sofle.keymap"
    ));

    #[test]
    fn the_repo_keymap_parses_cleanly() {
        let f = parse(REAL).unwrap();
        assert!(f.notes.is_empty(), "{:?}", f.notes);
        let names: Vec<_> = f.layers.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(&names[..5], ["BASE", "NAV", "CODE", "MEDIA", "SYS|NUM"]);
        assert!(f.layers.len() >= 9, "{names:?}");
        for l in &f.layers {
            assert_eq!(l.bindings.len(), 64, "{}", l.name);
            assert!(l.bindings.iter().all(FileBinding::resolved), "{}", l.name);
        }
        assert_eq!(f.move_val, 1200);
        assert_eq!(f.scrl_val, 25);
        for (display, label) in [
            ("home_row_mod_left", "hml"),
            ("home_row_mod_right_index", "hmr_idx"),
            ("ralt_mod_tap", "ralt_mt"),
            ("rsr_vol", "rsr_vol"),
            ("PIPE_OP", "M_PIPE"),
            ("GAME_MC_TO", "GAME_MC_TO"),
        ] {
            assert_eq!(
                f.behavior_names.get(display).map(String::as_str),
                Some(label)
            );
        }
        let b = &f.layers[0].bindings;
        assert_eq!(b[28].source, "&hml LALT S");
        assert_eq!(b[62].source, "&ralt_mt RALT F12");
        let sys = &f.layers[4].bindings;
        assert_eq!(sys[6].source, "&mmv MOVE_UP");
        assert_eq!(sys[6].value(0), 0xfb50); // MOVE_Y(-1200)
        assert_eq!(sys[14].source, "&rgb_ug RGB_SAI_CMD 0");
        assert_eq!(sys[1].source, "&bt BT_SEL 0");
        assert_eq!((sys[1].value(0), sys[1].value(1)), (3, 0));
    }

    #[test]
    fn truncated_files_never_panic() {
        // Every cut of the real file (on a char boundary) parses or fails
        // with a message; none panics.
        for end in (0..REAL.len())
            .step_by(37)
            .filter(|&e| REAL.is_char_boundary(e))
        {
            let _ = parse(&REAL[..end]);
        }
        for junk in [
            "<",
            "\"",
            "/*",
            "&",
            "#define",
            "a: { ; } };",
            "x = <(>;",
            "{{{{",
        ] {
            let _ = parse(junk);
        }
    }

    #[test]
    fn a_tenth_layer_is_just_another_layer() {
        let extra = r#"
        layer_9 {
            bindings = <&kp GRAVE &GAME_EXIT &to 9>;
            display-name = "MYGAME";
        };
    };
};"#;
        let end = REAL.rfind("};").unwrap();
        let end = REAL[..end].rfind("};").unwrap();
        let src = format!("{}{extra}", &REAL[..end]);
        let f = parse(&src).unwrap();
        let last = f.layers.last().unwrap();
        assert_eq!(
            (last.index, last.name.as_str()),
            (f.layers.len() - 1, "MYGAME")
        );
        assert_eq!(last.bindings[2].value(0), 9);
        assert!(f.notes.is_empty(), "{:?}", f.notes);
    }

    #[test]
    fn comments_strings_and_continuations() {
        assert_eq!(
            strip_comments("a // b\nc /* d\n e */ f \"// g\""),
            "a \nc \n  f \"// g\""
        );
        let f = parse(
            "#define ROW &kp A \\\n &kp B\n/ { keymap { compatible = \"zmk,keymap\"; l { bindings = <ROW>; }; }; };",
        )
        .unwrap();
        let src: Vec<_> = f.layers[0]
            .bindings
            .iter()
            .map(|b| b.source.as_str())
            .collect();
        assert_eq!(src, ["&kp A", "&kp B"]);
    }
}
