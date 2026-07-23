//! Recovery metadata for permissive Kotlin parsing.

#[allow(dead_code)]
pub(crate) const MAX_RECOVERY_ATTEMPTS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ByteRange {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

impl ByteRange {
    pub(crate) fn new(start: usize, end: usize) -> Option<Self> {
        (start <= end).then_some(Self { start, end })
    }

    #[allow(dead_code)]
    pub(crate) fn contains(self, byte: usize) -> bool {
        self.start <= byte && byte < self.end
    }

    #[allow(dead_code)]
    pub(crate) fn contains_node(self, node: tree_sitter::Node<'_>) -> bool {
        self.start <= node.start_byte() && node.end_byte() <= self.end
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
pub(crate) enum SyntaxFamily {
    SuspendLambda,
    WhenGuard,
    AnnotatedFunctionType,
    ExplicitBackingField,
    ContextParameter,
    ContextReceiver,
    AnnotatedTypeArgument,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum ComponentScopePolicy {
    Inherit,
    ComposableLambda,
    Exclude,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyntaxRegion {
    pub(crate) source: ByteRange,
    pub(crate) body: Option<ByteRange>,
    pub(crate) family: SyntaxFamily,
    pub(crate) component_scope: ComponentScopePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyntaxProblem {
    pub(crate) range: ByteRange,
    pub(crate) line: u32,
    pub(crate) column: u32,
    pub(crate) family: SyntaxFamily,
    pub(crate) recovered_later_source: bool,
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct ParsePass {
    pub(crate) tree: tree_sitter::Tree,
    pub(crate) clean: Vec<ByteRange>,
    pub(crate) priority: u16,
}

pub(crate) fn merge_clean_ranges(mut ranges: Vec<ByteRange>) -> Vec<ByteRange> {
    ranges.sort();

    let mut merged: Vec<ByteRange> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(previous) if range.start <= previous.end => {
                previous.end = previous.end.max(range.end);
            }
            _ => merged.push(range),
        }
    }

    merged
}
