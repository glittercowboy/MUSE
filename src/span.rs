/// Byte-offset span used by AST nodes and diagnostics.
///
/// This is a type alias for chumsky's `SimpleSpan`, which is a `Range<usize>`
/// representing byte offsets into the source text.
pub type Span = chumsky::span::SimpleSpan<usize>;
