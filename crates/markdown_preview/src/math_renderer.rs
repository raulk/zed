use crate::markdown_elements::{
    MarkdownParagraph, MarkdownParagraphChunk, ParsedMarkdownElement, ParsedMarkdownMath,
    ParsedMarkdownMathBlock, ParsedMarkdownText,
};
use gpui::SharedString;

/// Process markdown elements to extract and render math expressions
pub fn process_math_expressions(elements: Vec<ParsedMarkdownElement>) -> Vec<ParsedMarkdownElement> {
    elements
        .into_iter()
        .flat_map(|element| process_element(element))
        .collect()
}

fn process_element(element: ParsedMarkdownElement) -> Vec<ParsedMarkdownElement> {
    match element {
        ParsedMarkdownElement::Paragraph(chunks) => {
            let processed = process_paragraph(chunks);
            if processed.is_empty() {
                vec![]
            } else {
                vec![ParsedMarkdownElement::Paragraph(processed)]
            }
        }
        ParsedMarkdownElement::Heading(mut heading) => {
            heading.contents = process_paragraph(heading.contents);
            vec![ParsedMarkdownElement::Heading(heading)]
        }
        ParsedMarkdownElement::ListItem(mut item) => {
            item.content = process_math_expressions(item.content);
            vec![ParsedMarkdownElement::ListItem(item)]
        }
        ParsedMarkdownElement::BlockQuote(mut quote) => {
            quote.children = process_math_expressions(quote.children);
            vec![ParsedMarkdownElement::BlockQuote(quote)]
        }
        ParsedMarkdownElement::Table(mut table) => {
            table.header.children = table
                .header
                .children
                .into_iter()
                .map(process_paragraph)
                .collect();
            table.body = table
                .body
                .into_iter()
                .map(|mut row| {
                    row.children = row.children.into_iter().map(process_paragraph).collect();
                    row
                })
                .collect();
            vec![ParsedMarkdownElement::Table(table)]
        }
        other => vec![other],
    }
}

fn process_paragraph(chunks: MarkdownParagraph) -> MarkdownParagraph {
    let mut result = Vec::new();

    for chunk in chunks {
        match chunk {
            MarkdownParagraphChunk::Text(text) => {
                let processed = extract_math_from_text(text);
                result.extend(processed);
            }
            other => result.push(other),
        }
    }

    result
}

fn extract_math_from_text(text: ParsedMarkdownText) -> Vec<MarkdownParagraphChunk> {
    let contents = text.contents.as_ref();
    let mut result = Vec::new();
    let mut current_pos = 0;
    let source_start = text.source_range.start;

    // First, check for block math ($$...$$)
    let mut chars = contents.chars().peekable();
    let mut temp_pos = 0;
    let mut in_block_math = false;
    let mut block_start = 0;

    // Collect positions of $$ delimiters
    let mut block_delimiters = Vec::new();
    while let Some(c) = chars.next() {
        if c == '$' {
            if let Some(&'$') = chars.peek() {
                chars.next(); // consume second $
                if in_block_math {
                    // End of block math
                    block_delimiters.push((block_start, temp_pos + 2, true));
                    in_block_math = false;
                } else {
                    // Start of block math
                    block_start = temp_pos;
                    in_block_math = true;
                }
                temp_pos += 2;
                continue;
            }
        }
        temp_pos += c.len_utf8();
    }

    // Process text with block math expressions
    for (start, end, is_end) in &block_delimiters {
        if !is_end {
            continue;
        }

        let block_start_pos = block_delimiters
            .iter()
            .find(|(s, _, _)| s < start && !block_delimiters.iter().any(|(_, e, ie)| *ie && e == s))
            .map(|(s, _, _)| *s);

        if let Some(block_start_pos) = block_start_pos {
            // Add text before the block math
            if current_pos < block_start_pos {
                let before_text = &contents[current_pos..block_start_pos];
                if !before_text.is_empty() {
                    result.extend(process_inline_math(ParsedMarkdownText {
                        source_range: (source_start + current_pos)..(source_start + block_start_pos),
                        contents: before_text.into(),
                        highlights: Vec::new(),
                        region_ranges: Vec::new(),
                        regions: Vec::new(),
                    }));
                }
            }

            current_pos = *end;
        }
    }

    // Process remaining text with inline math
    if current_pos < contents.len() {
        let remaining = &contents[current_pos..];
        result.extend(process_inline_math(ParsedMarkdownText {
            source_range: (source_start + current_pos)..(source_start + contents.len()),
            contents: remaining.into(),
            highlights: text.highlights.clone(),
            region_ranges: text.region_ranges.clone(),
            regions: text.regions.clone(),
        }));
    } else if result.is_empty() {
        // If nothing was processed, return the original text
        result.push(MarkdownParagraphChunk::Text(text));
    }

    result
}

fn process_inline_math(text: ParsedMarkdownText) -> Vec<MarkdownParagraphChunk> {
    let contents = text.contents.as_ref();
    let mut result = Vec::new();
    let mut current_pos = 0;
    let source_start = text.source_range.start;

    // Find inline math expressions ($...$)
    let mut chars = contents.chars().enumerate().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '$' {
            // Check if this is not the start of $$
            if let Some(&(_, '$')) = chars.peek() {
                continue; // Skip, this is part of $$
            }

            // Find the closing $
            let start_pos = i;
            let mut end_pos = None;
            let mut escaped = false;

            while let Some((j, next_c)) = chars.next() {
                if next_c == '\\' && !escaped {
                    escaped = true;
                    continue;
                }
                if next_c == '$' && !escaped {
                    // Check if this is not the start of another $$
                    if let Some(&(_, '$')) = chars.peek() {
                        escaped = false;
                        continue; // This is $$, not a closing $
                    }
                    end_pos = Some(j);
                    break;
                }
                escaped = false;
            }

            if let Some(end_pos) = end_pos {
                // Add text before the math expression
                if current_pos < start_pos {
                    let before_text = &contents[current_pos..start_pos];
                    result.push(MarkdownParagraphChunk::Text(ParsedMarkdownText {
                        source_range: (source_start + current_pos)..(source_start + start_pos),
                        contents: before_text.into(),
                        highlights: Vec::new(),
                        region_ranges: Vec::new(),
                        regions: Vec::new(),
                    }));
                }

                // Extract math content (without the $ delimiters)
                let math_content = &contents[start_pos + 1..end_pos];
                let math_svg = render_math(math_content, false);

                result.push(MarkdownParagraphChunk::InlineMath(ParsedMarkdownMath {
                    source_range: (source_start + start_pos)..(source_start + end_pos + 1),
                    contents: math_content.into(),
                    svg: math_svg,
                }));

                current_pos = end_pos + 1;
            }
        }
    }

    // Add remaining text
    if current_pos < contents.len() {
        let remaining = &contents[current_pos..];
        result.push(MarkdownParagraphChunk::Text(ParsedMarkdownText {
            source_range: (source_start + current_pos)..(source_start + contents.len()),
            contents: remaining.into(),
            highlights: text.highlights,
            region_ranges: text.region_ranges,
            regions: text.regions,
        }));
    } else if result.is_empty() {
        result.push(MarkdownParagraphChunk::Text(text));
    }

    result
}

fn render_math(latex: &str, display: bool) -> Option<SharedString> {
    match mathjax_svg::render(latex, display) {
        Ok(svg) => Some(svg.into()),
        Err(err) => {
            log::warn!("Failed to render math expression '{}': {}", latex, err);
            None
        }
    }
}

pub fn extract_block_math(elements: Vec<ParsedMarkdownElement>) -> Vec<ParsedMarkdownElement> {
    let mut result = Vec::new();

    for element in elements {
        match element {
            ParsedMarkdownElement::Paragraph(chunks) => {
                let (block_math, remaining) = extract_block_math_from_paragraph(chunks);
                result.extend(block_math);
                if !remaining.is_empty() {
                    result.push(ParsedMarkdownElement::Paragraph(remaining));
                }
            }
            other => result.push(other),
        }
    }

    result
}

fn extract_block_math_from_paragraph(
    chunks: MarkdownParagraph,
) -> (Vec<ParsedMarkdownElement>, MarkdownParagraph) {
    let mut block_math = Vec::new();
    let mut remaining = Vec::new();

    for chunk in chunks {
        match chunk {
            MarkdownParagraphChunk::Text(text) => {
                let (math_blocks, text_chunks) = extract_block_math_from_text(text);
                block_math.extend(math_blocks);
                remaining.extend(text_chunks);
            }
            other => remaining.push(other),
        }
    }

    (block_math, remaining)
}

fn extract_block_math_from_text(
    text: ParsedMarkdownText,
) -> (Vec<ParsedMarkdownElement>, Vec<MarkdownParagraphChunk>) {
    let contents = text.contents.as_ref();
    let mut math_blocks = Vec::new();
    let mut text_chunks = Vec::new();
    let mut current_pos = 0;
    let source_start = text.source_range.start;

    // Find block math expressions ($$...$$)
    let mut pos = 0;
    while pos < contents.len() {
        if let Some(start) = contents[pos..].find("$$") {
            let abs_start = pos + start;

            // Find the closing $$
            if let Some(end) = contents[abs_start + 2..].find("$$") {
                let abs_end = abs_start + 2 + end;

                // Add text before the block math
                if current_pos < abs_start {
                    let before_text = &contents[current_pos..abs_start];
                    if !before_text.trim().is_empty() {
                        text_chunks.push(MarkdownParagraphChunk::Text(ParsedMarkdownText {
                            source_range: (source_start + current_pos)..(source_start + abs_start),
                            contents: before_text.into(),
                            highlights: Vec::new(),
                            region_ranges: Vec::new(),
                            regions: Vec::new(),
                        }));
                    }
                }

                // Extract math content (without the $$ delimiters)
                let math_content = &contents[abs_start + 2..abs_end];
                let math_svg = render_math(math_content, true);

                math_blocks.push(ParsedMarkdownElement::MathBlock(ParsedMarkdownMathBlock {
                    source_range: (source_start + abs_start)..(source_start + abs_end + 2),
                    contents: math_content.into(),
                    svg: math_svg,
                }));

                current_pos = abs_end + 2;
                pos = current_pos;
            } else {
                pos = abs_start + 2;
            }
        } else {
            break;
        }
    }

    // Add remaining text
    if current_pos < contents.len() {
        let remaining = &contents[current_pos..];
        if !remaining.trim().is_empty() {
            text_chunks.push(MarkdownParagraphChunk::Text(ParsedMarkdownText {
                source_range: (source_start + current_pos)..(source_start + contents.len()),
                contents: remaining.into(),
                highlights: text.highlights,
                region_ranges: text.region_ranges,
                regions: text.regions,
            }));
        }
    } else if math_blocks.is_empty() {
        text_chunks.push(MarkdownParagraphChunk::Text(text));
    }

    (math_blocks, text_chunks)
}
