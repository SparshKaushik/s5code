//! A keybinding's `when` condition (`KeybindingWhenNode`): the shared parser, web's formatter,
//! and the condition editor's tree edits (`WhenExpressionNodeEditor`).
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum When {
    Id(String),
    Not(Box<When>),
    And(Box<When>, Box<When>),
    Or(Box<When>, Box<When>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    And,
    Or,
}

impl Op {
    pub fn name(self) -> &'static str {
        match self {
            Op::And => "and",
            Op::Or => "or",
        }
    }
    fn flip(self) -> Op {
        match self {
            Op::And => Op::Or,
            Op::Or => Op::And,
        }
    }
}

/// `MAX_WHEN_EXPRESSION_DEPTH` in contracts.
const MAX_DEPTH: usize = 64;
const CORE_VARIABLES: [&str; 6] = ["terminalFocus", "terminalOpen", "isWeb", "isDesktop", "true", "false"];
pub const DEFAULT_VARIABLE: &str = "terminalFocus";
pub const PARSE_ERROR: &str = "Use variables with !, &&, ||, and parentheses.";

impl When {
    pub fn id(name: impl Into<String>) -> When {
        When::Id(name.into())
    }
    pub fn not(node: When) -> When {
        When::Not(Box::new(node))
    }
    pub fn join(op: Op, left: When, right: When) -> When {
        match op {
            Op::And => When::And(Box::new(left), Box::new(right)),
            Op::Or => When::Or(Box::new(left), Box::new(right)),
        }
    }
    pub fn op(&self) -> Option<Op> {
        match self {
            When::And(..) => Some(Op::And),
            When::Or(..) => Some(Op::Or),
            _ => None,
        }
    }
    /// A resolved rule's `whenAst`.
    pub fn from_json(v: &Value) -> Option<When> {
        match v["type"].as_str()? {
            "identifier" => Some(When::id(v["name"].as_str()?)),
            "not" => Some(When::not(When::from_json(&v["node"])?)),
            "and" => Some(When::join(Op::And, When::from_json(&v["left"])?, When::from_json(&v["right"])?)),
            "or" => Some(When::join(Op::Or, When::from_json(&v["left"])?, When::from_json(&v["right"])?)),
            _ => None,
        }
    }
}

/// `whenAstToExpression`: only `&&`/`||` operands get parentheses.
pub fn expression(node: Option<&When>) -> String {
    node.map(render).unwrap_or_default()
}

fn render(node: &When) -> String {
    match node {
        When::Id(name) => name.clone(),
        When::Not(inner) => format!("!{}", wrap(inner)),
        When::And(left, right) => format!("{} && {}", wrap(left), wrap(right)),
        When::Or(left, right) => format!("{} || {}", wrap(left), wrap(right)),
    }
}

fn wrap(node: &When) -> String {
    match node {
        When::Id(_) | When::Not(_) => render(node),
        _ => format!("({})", render(node)),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    And,
    Or,
    Not,
    Open,
    Close,
    Id(String),
}

fn tokenize(expression: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = expression.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let single = match c {
            '!' => Some(Token::Not),
            '(' => Some(Token::Open),
            ')' => Some(Token::Close),
            _ => None,
        };
        if c.is_whitespace() {
            i += 1;
        } else if c == '&' && next == Some('&') {
            tokens.push(Token::And);
            i += 2;
        } else if c == '|' && next == Some('|') {
            tokens.push(Token::Or);
            i += 2;
        } else if let Some(token) = single {
            tokens.push(token);
            i += 1;
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '_' | '.' | '-')) {
                i += 1;
            }
            tokens.push(Token::Id(chars[start..i].iter().collect()));
        } else {
            return None;
        }
    }
    Some(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }
    fn primary(&mut self, depth: usize) -> Option<When> {
        if depth > MAX_DEPTH {
            return None;
        }
        match self.peek()?.clone() {
            Token::Id(name) => {
                self.at += 1;
                Some(When::Id(name))
            }
            Token::Open => {
                self.at += 1;
                let node = self.or(depth + 1)?;
                if self.peek() != Some(&Token::Close) {
                    return None;
                }
                self.at += 1;
                Some(node)
            }
            _ => None,
        }
    }
    fn unary(&mut self, depth: usize) -> Option<When> {
        let mut nots = 0;
        while self.peek() == Some(&Token::Not) {
            self.at += 1;
            nots += 1;
            if nots > MAX_DEPTH {
                return None;
            }
        }
        let mut node = self.primary(depth)?;
        for _ in 0..nots {
            node = When::not(node);
        }
        Some(node)
    }
    fn and(&mut self, depth: usize) -> Option<When> {
        let mut left = self.unary(depth)?;
        while self.peek() == Some(&Token::And) {
            self.at += 1;
            let right = self.unary(depth)?;
            left = When::join(Op::And, left, right);
        }
        Some(left)
    }
    fn or(&mut self, depth: usize) -> Option<When> {
        let mut left = self.and(depth)?;
        while self.peek() == Some(&Token::Or) {
            self.at += 1;
            let right = self.and(depth)?;
            left = When::join(Op::Or, left, right);
        }
        Some(left)
    }
}

/// `parseKeybindingWhenExpression`.
pub fn parse(expression: &str) -> Option<When> {
    let tokens = tokenize(expression)?;
    if tokens.is_empty() {
        return None;
    }
    let mut parser = Parser { tokens, at: 0 };
    let ast = parser.or(0)?;
    (parser.at == parser.tokens.len()).then_some(ast)
}

/// `parseWhenExpressionDraft`: empty is "always"; anything else must parse.
pub fn parse_draft(expression: &str) -> Result<Option<When>, &'static str> {
    let trimmed = expression.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    parse(trimmed).map(Some).ok_or(PARSE_ERROR)
}

/// A single variable, possibly negated: its name and whether it is negated.
pub fn condition(node: &When) -> Option<(&str, bool)> {
    match node {
        When::Id(name) => Some((name.as_str(), false)),
        When::Not(inner) => match inner.as_ref() {
            When::Id(name) => Some((name.as_str(), true)),
            _ => None,
        },
        _ => None,
    }
}

/// `whenNodeRemoveLabel`.
pub fn remove_label(node: &When, depth: usize) -> &'static str {
    if depth == 0 {
        "Clear all conditions"
    } else if condition(node).is_some() {
        "Remove condition"
    } else {
        "Remove group and its conditions"
    }
}

pub fn flatten(node: &When, op: Op) -> Vec<When> {
    match (node, op) {
        (When::And(left, right), Op::And) | (When::Or(left, right), Op::Or) => {
            let mut children = flatten(left, op);
            children.extend(flatten(right, op));
            children
        }
        _ => vec![node.clone()],
    }
}

pub fn group(children: Vec<When>, op: Op) -> Option<When> {
    let mut children = children.into_iter();
    let first = children.next()?;
    Some(children.fold(first, |left, right| When::join(op, left, right)))
}

fn default_condition() -> When {
    When::id(DEFAULT_VARIABLE)
}

fn default_group(op: Op) -> When {
    When::join(op, default_condition(), When::not(default_condition()))
}

/// The header's "Condition" and "Group" buttons: and-ed onto whatever is there.
pub fn add_root(root: Option<&When>, as_group: bool) -> When {
    let added = if as_group { default_group(Op::Or) } else { default_condition() };
    match root {
        Some(root) => When::join(Op::And, root.clone(), added),
        None => added,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Negate(bool),
    Variable(String),
    Operator(Op),
    AddCondition,
    AddGroup,
    Remove,
}

/// Applies `edit` to the node at `path`: indices into each group's flattened children, `0` for
/// a negated group's body. `None` means the whole condition was removed.
pub fn edit(root: &When, path: &[usize], edit: &Edit) -> Option<When> {
    let Some((index, rest)) = path.split_first() else {
        return edit_here(root, edit);
    };
    match root {
        When::Not(inner) => Some(match self::edit(inner, rest, edit) {
            Some(next) => When::not(next),
            None => root.clone(),
        }),
        When::And(..) | When::Or(..) => {
            let op = root.op()?;
            let mut children = flatten(root, op);
            let Some(child) = children.get(*index) else {
                return Some(root.clone());
            };
            match self::edit(child, rest, edit) {
                Some(next) => children[*index] = next,
                None => {
                    children.remove(*index);
                }
            }
            Some(group(children, op).unwrap_or_else(default_condition))
        }
        When::Id(_) => Some(root.clone()),
    }
}

fn edit_here(node: &When, edit: &Edit) -> Option<When> {
    Some(match edit {
        Edit::Remove => return None,
        Edit::Negate(on) => match (condition(node), node) {
            (Some((name, _)), _) if *on => When::not(When::id(name)),
            (Some((name, _)), _) => When::id(name),
            (None, When::Not(inner)) if !*on => inner.as_ref().clone(),
            (None, When::Not(_)) => node.clone(),
            (None, _) if *on => When::not(node.clone()),
            (None, _) => node.clone(),
        },
        Edit::Variable(name) => match condition(node) {
            Some((_, true)) => When::not(When::id(name.clone())),
            Some((_, false)) => When::id(name.clone()),
            None => node.clone(),
        },
        Edit::Operator(next) => match node.op() {
            Some(current) if current != *next => group(flatten(node, current), *next)?,
            _ => node.clone(),
        },
        Edit::AddCondition | Edit::AddGroup => match node.op() {
            Some(op) => {
                let mut children = flatten(node, op);
                children.push(if *edit == Edit::AddGroup { default_group(op.flip()) } else { default_condition() });
                group(children, op)?
            }
            None => node.clone(),
        },
    })
}

fn identifiers(node: &When, out: &mut Vec<String>) {
    match node {
        When::Id(name) => {
            if !out.contains(name) {
                out.push(name.clone());
            }
        }
        When::Not(inner) => identifiers(inner, out),
        When::And(left, right) | When::Or(left, right) => {
            identifiers(left, out);
            identifiers(right, out);
        }
    }
}

pub fn locale_order(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
}

/// `buildWhenVariableOptions`: the core variables in order, then the defaults' alphabetically.
pub fn variable_options(defaults: impl IntoIterator<Item = When>) -> Vec<String> {
    let mut known: Vec<String> = CORE_VARIABLES.iter().map(|v| v.to_string()).collect();
    for ast in defaults {
        identifiers(&ast, &mut known);
    }
    let core = |v: &str| CORE_VARIABLES.iter().position(|c| *c == v);
    known.sort_by(|a, b| match (core(a.as_str()), core(b.as_str())) {
        (None, None) => locale_order(a, b),
        (a, b) => a.unwrap_or(usize::MAX).cmp(&b.unwrap_or(usize::MAX)),
    });
    known
}

/// `unknownWhenVariables`.
pub fn unknown(node: Option<&When>, known: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(node) = node {
        identifiers(node, &mut names);
    }
    names.retain(|name| !known.contains(name));
    names.sort();
    names
}
