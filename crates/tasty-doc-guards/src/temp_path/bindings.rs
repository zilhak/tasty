//! Lexical visibility for simple local bindings; this is not Rust type/dataflow analysis.
//! Character offsets keep the raw and masked copies aligned even inside Unicode strings.
use super::{Axes, UNIQ_TOKENS, axes_of};

struct Token {
    text: String,
    start: usize,
    end: usize,
}

struct Scope {
    parent: usize,
    function: usize,
}

struct Binding {
    name: String,
    name_start: usize,
    visible_from: usize,
    scope: usize,
    axes: Axes,
}

pub(super) struct Bindings {
    code: Vec<char>,
    raw: Vec<char>,
    comments: Vec<char>,
    lines: Vec<usize>,
    scope_at: Vec<usize>,
    scopes: Vec<Scope>,
    bindings: Vec<Binding>,
}

fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn tokens(code: &[char]) -> Vec<Token> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < code.len() {
        if code[i].is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        if word(code[start]) {
            while i < code.len() && word(code[i]) {
                i += 1;
            }
        }
        out.push(Token {
            text: code[start..i].iter().collect(),
            start,
            end: i,
        });
    }
    out
}

impl Bindings {
    pub(super) fn new(code: &[&str], raw: &[&str], comments: &[&str]) -> Self {
        let mut lines = vec![0];
        for line in code {
            lines.push(lines.last().unwrap() + line.chars().count() + 1);
        }
        let code: Vec<char> = code.join("\n").chars().collect();
        let raw: Vec<char> = raw.join("\n").chars().collect();
        let mut comments: Vec<char> = comments.join("\n").chars().collect();
        if comments.len() != raw.len() {
            // Caller-chain classification deliberately disables reason comments.
            // Reference masking still needs the real literal/comment boundary.
            comments = crate::source_text::mask_literals(&raw.iter().collect::<String>())
                .chars()
                .collect();
        }
        let tokens = tokens(&code);
        let mut scopes = vec![Scope {
            parent: 0,
            function: 0,
        }];
        let mut scope_at = vec![0; code.len()];
        let (mut scope, mut previous, mut function_body) = (0, 0, false);
        for (i, token) in tokens.iter().enumerate() {
            scope_at[previous..token.end].fill(scope);
            match token.text.as_str() {
                "fn" if tokens
                    .get(i + 1)
                    .is_some_and(|t| t.text.chars().next().is_some_and(word) && t.text != "_") =>
                {
                    function_body = true
                }
                ";" => function_body = false,
                "{" => {
                    let id = scopes.len();
                    scopes.push(Scope {
                        parent: scope,
                        function: if function_body {
                            id
                        } else {
                            scopes[scope].function
                        },
                    });
                    scope = id;
                    function_body = false;
                }
                "}" => scope = scopes[scope].parent,
                _ => {}
            }
            previous = token.end;
        }
        scope_at[previous..].fill(scope);
        let mut bindings = Vec::new();
        for (i, token) in tokens.iter().enumerate() {
            if token.text != "let" {
                continue;
            }
            let name_idx = i + 1 + usize::from(tokens.get(i + 1).is_some_and(|t| t.text == "mut"));
            let Some(name) = tokens.get(name_idx) else {
                continue;
            };
            if !name.text.chars().next().is_some_and(word)
                || !tokens
                    .get(name_idx + 1)
                    .is_some_and(|t| matches!(t.text.as_str(), "=" | ":" | ";"))
            {
                continue;
            }
            let (mut depth, mut end) = (0i32, None);
            for t in &tokens[name_idx + 1..] {
                match t.text.as_str() {
                    "(" | "[" | "{" => depth += 1,
                    ")" | "]" | "}" => depth -= 1,
                    ";" if depth == 0 => {
                        end = Some(t);
                        break;
                    }
                    _ => {}
                }
                if depth < 0 {
                    break;
                }
            }
            let Some(end) = end else { continue };
            let initializer: String = code[name.end..end.start].iter().collect();
            let axes = if UNIQ_TOKENS.iter().any(|t| initializer.contains(t)) {
                axes_of(&initializer)
            } else {
                Axes::default()
            };
            // Keep zero-axis bindings too: a shadow must hide an earlier uniquifier.
            bindings.push(Binding {
                name: name.text.clone(),
                name_start: name.start,
                visible_from: end.end,
                scope: scope_at[token.start],
                axes,
            });
        }
        Self {
            code,
            raw,
            comments,
            lines,
            scope_at,
            scopes,
            bindings,
        }
    }

    fn ancestor(&self, ancestor: usize, mut scope: usize) -> bool {
        loop {
            if ancestor == scope {
                return true;
            }
            if scope == 0 {
                return false;
            }
            scope = self.scopes[scope].parent;
        }
    }

    fn visible(&self, name: &str, at: usize) -> Axes {
        let scope = self.scope_at[at];
        self.bindings
            .iter()
            .rev()
            .find(|b| {
                b.name == name
                    && b.visible_from <= at
                    && self.scopes[b.scope].function == self.scopes[scope].function
                    && self.ancestor(b.scope, scope)
            })
            .map_or(Axes::default(), |b| b.axes)
    }

    /// Keep the existing direct-token window, but never borrow from another function.
    /// Binding references additionally obey declaration order, blocks, and shadowing.
    pub(super) fn axes_on_line(&self, site: usize, line: usize) -> Axes {
        let site_start = self.lines[site];
        let site_end = self.lines[site + 1].min(self.code.len());
        let site_text: String = self.code[site_start..site_end].iter().collect();
        let at = site_start
            + site_text
                .find("temp_dir()")
                .map_or(0, |i| site_text[..i].chars().count());
        let site_scope = self.scope_at[at];
        let function = self.scopes[site_scope].function;
        let lo = self.lines[line];
        let hi = self.lines[line + 1].min(self.code.len());
        let same_function = |pos: usize| {
            self.scopes[self.scope_at[pos]].function == function
                && self.ancestor(site_scope, self.scope_at[pos])
        };
        let direct: String = (lo..hi)
            .map(|p| if same_function(p) { self.code[p] } else { ' ' })
            .collect();
        let mut axes = axes_of(&direct);
        let mut i = lo;
        while i < hi {
            if !word(self.raw[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < hi && word(self.raw[i]) {
                i += 1;
            }
            if !same_function(start) || self.bindings.iter().any(|b| b.name_start == start) {
                continue;
            }
            // Literal interpolation remains visible; comment-only references do not.
            if self.code[start..i].iter().all(|c| c.is_whitespace())
                && self.comments[start..i].iter().any(|c| !c.is_whitespace())
            {
                continue;
            }
            axes = axes.union(self.visible(&self.raw[start..i].iter().collect::<String>(), start));
        }
        axes
    }
}
