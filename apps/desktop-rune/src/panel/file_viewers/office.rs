use std::collections::HashMap;
use std::io::{Cursor, Read};

use roxmltree::{Document, Node, ParsingOptions};
use rune::{El, ScrollSpec, col, el, row, text};

use crate::panel::file_preview::table_number;
use crate::theme::{self, T};
use crate::{App, icons, kit};

const MAX_PART: u64 = 8 * 1024 * 1024;
const MAX_TOTAL: u64 = 50 * 1024 * 1024;
const MAX_PAGES: usize = 100;
const MAX_ROWS: usize = 200;
const MAX_COLS: usize = 32;
const MAX_BLOCKS: usize = 500;

#[derive(Clone, Copy)]
pub(in crate::panel) enum Format {
    Word,
    Sheet,
    Slides,
}

impl Format {
    pub(super) fn from_path(path: &str) -> Option<Self> {
        match path.rsplit_once('.')?.1.to_ascii_lowercase().as_str() {
            "docx" => Some(Self::Word),
            "xlsx" => Some(Self::Sheet),
            "pptx" => Some(Self::Slides),
            _ => None,
        }
    }
}

enum Block {
    Paragraph { text: String, heading: bool },
    Table(Vec<Vec<String>>),
}

struct Page {
    blocks: Vec<Block>,
}

struct Sheet {
    name: String,
    rows: Vec<Vec<String>>,
    columns: usize,
}

enum DocumentView {
    Word(Page),
    Sheet(Vec<Sheet>),
    Slides(Vec<Page>),
}

pub(super) struct Office {
    document: DocumentView,
    active: usize,
    limited: bool,
}

struct Package<'a> {
    zip: zip::ZipArchive<Cursor<&'a [u8]>>,
    read: u64,
    limited: bool,
}

fn xml(source: &str) -> Result<Document<'_>, String> {
    Document::parse_with_options(source, ParsingOptions {
        allow_dtd: false,
        nodes_limit: 400_000,
    }).map_err(|_| "Could not read the Office document XML.".into())
}

impl Package<'_> {
    fn part(&mut self, name: &str) -> Result<String, String> {
        let mut part = self.zip.by_name(name).map_err(|_| "An Office document part is missing or damaged.")?;
        let size = part.size();
        if size > MAX_PART || self.read.saturating_add(size) > MAX_TOTAL
            || size > part.compressed_size().saturating_mul(100)
        {
            return Err("The expanded Office document exceeds the preview limit.".into());
        }
        let mut source = String::new();
        (&mut part).take(size + 1).read_to_string(&mut source).map_err(|_| "Could not read the Office document XML.")?;
        if source.len() as u64 != size {
            return Err("An Office document part is damaged.".into());
        }
        self.read += size;
        Ok(source)
    }

    fn related_parts(&mut self, base: &str, part: &str) -> Result<HashMap<String, String>, String> {
        let source = self.part(&format!("{base}/_rels/{part}.rels"))?;
        let doc = xml(&source)?;
        Ok(doc.root_element().children().filter(|n| tag(*n, "Relationship"))
            .filter(|n| n.attribute("TargetMode") != Some("External"))
            .filter_map(|n| Some((n.attribute("Id")?.to_owned(), relation_path(base, n.attribute("Target")?)?)))
            .collect())
    }
}

fn relation_path(base: &str, target: &str) -> Option<String> {
    let path = if let Some(absolute) = target.strip_prefix('/') {
        absolute.to_owned()
    } else {
        format!("{base}/{target}")
    };
    (path.starts_with(&format!("{base}/")) && !path.contains(['\\', ':', '\0', '#', '?'])
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))).then_some(path)
}

fn tag(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name
}

fn attr<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attributes().find(|attr| attr.name() == name).map(|attr| attr.value())
}

fn relationship_id<'a>(node: Node<'a, '_>) -> Option<&'a str> {
    node.attributes().find(|attr| attr.name() == "id" && attr.namespace().is_some_and(|ns| ns.ends_with("/relationships")))
        .map(|attr| attr.value())
}

fn runs(node: Node<'_, '_>) -> String {
    let mut value = String::new();
    for child in node.descendants().filter(Node::is_element) {
        match child.tag_name().name() {
            "t" => value.push_str(child.text().unwrap_or("")),
            "tab" => value.push('\t'),
            "br" | "cr" => value.push('\n'),
            _ => {}
        }
    }
    value.truncate(value.floor_char_boundary(value.len().min(16_384)));
    value
}

fn paragraph(node: Node<'_, '_>, slide: bool) -> Option<Block> {
    let text = runs(node);
    if text.trim().is_empty() {
        return None;
    }
    let heading = node.descendants().any(|n| tag(n, "pStyle") && attr(n, "val").is_some_and(|style| style.starts_with("Heading") || style == "Title"));
    Some(Block::Paragraph { text, heading: slide || heading })
}

fn table(node: Node<'_, '_>, limited: &mut bool) -> Block {
    let mut rows = Vec::new();
    for line in node.children().filter(|n| tag(*n, "tr")) {
        if rows.len() == MAX_ROWS {
            *limited = true;
            break;
        }
        let cells = line.children().filter(|n| tag(*n, "tc")).take(MAX_COLS + 1)
            .map(|cell| cell.descendants().filter(|n| tag(*n, "p")).map(runs).collect::<Vec<_>>().join("\n"))
            .collect::<Vec<_>>();
        *limited |= cells.len() > MAX_COLS;
        rows.push(cells.into_iter().take(MAX_COLS).collect());
    }
    Block::Table(rows)
}

fn word(source: &str, limited: &mut bool) -> Result<Page, String> {
    let doc = xml(source)?;
    let body = doc.root_element().children().find(|n| tag(*n, "body")).ok_or("This Word document has no body.")?;
    let mut blocks = Vec::new();
    for node in body.children().filter(Node::is_element) {
        if blocks.len() == MAX_BLOCKS {
            *limited = true;
            break;
        }
        if tag(node, "tbl") {
            blocks.push(table(node, limited));
        } else if tag(node, "p") {
            blocks.extend(paragraph(node, false));
        }
    }
    Ok(Page { blocks })
}

fn slide(source: &str, limited: &mut bool) -> Result<Page, String> {
    let doc = xml(source)?;
    let mut blocks = Vec::new();
    for node in doc.root_element().descendants().filter(|node| tag(*node, "sp") || tag(*node, "tbl")) {
        if blocks.len() == MAX_BLOCKS {
            *limited = true;
            break;
        }
        if tag(node, "tbl") {
            blocks.push(table(node, limited));
        } else {
            let title = node.descendants().any(|n| tag(n, "ph") && matches!(n.attribute("type"), Some("title" | "ctrTitle")));
            blocks.extend(node.descendants().filter(|n| tag(*n, "p")).take(MAX_BLOCKS - blocks.len())
                .filter_map(|n| paragraph(n, title)));
        }
    }
    Ok(Page { blocks })
}

fn cell_column(address: &str) -> Option<usize> {
    let letters = address.bytes().take_while(u8::is_ascii_uppercase);
    let column = letters.take(4).try_fold(0usize, |col, letter| col.checked_mul(26)?.checked_add((letter - b'A' + 1) as usize))?;
    column.checked_sub(1)
}

fn cell_value(cell: Node<'_, '_>, shared: &[String]) -> String {
    let raw = cell.children().find(|n| tag(*n, "v")).and_then(|value| value.text()).unwrap_or("");
    let value = match cell.attribute("t") {
        Some("inlineStr") => return runs(cell),
        Some("s") => raw.parse::<usize>().ok().and_then(|n| shared.get(n)).map(String::as_str).unwrap_or(""),
        Some("b") if raw == "1" => "TRUE",
        Some("b") if raw == "0" => "FALSE",
        _ => raw,
    };
    value[..value.floor_char_boundary(value.len().min(8192))].to_owned()
}

fn worksheet(source: &str, name: String, shared: &[String], limited: &mut bool) -> Result<Sheet, String> {
    let doc = xml(source)?;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut columns = 1;
    let data = doc.root_element().children().find(|n| tag(*n, "sheetData"));
    for line in data.into_iter().flat_map(|n| n.children().filter(|n| tag(*n, "row"))) {
        let index = line.attribute("r").and_then(|v| v.parse::<usize>().ok()).and_then(|n| n.checked_sub(1)).unwrap_or(rows.len());
        if index >= MAX_ROWS {
            *limited = true;
            continue;
        }
        rows.resize_with(rows.len().max(index + 1), Vec::new);
        let row = &mut rows[index];
        for cell in line.children().filter(|n| tag(*n, "c")) {
            let column = cell.attribute("r").and_then(cell_column).unwrap_or(row.len());
            if column >= MAX_COLS {
                *limited = true;
                continue;
            }
            row.resize_with(row.len().max(column + 1), String::new);
            row[column] = cell_value(cell, shared);
            columns = columns.max(column + 1);
        }
    }
    Ok(Sheet { name, rows, columns })
}

impl Office {
    pub(super) fn open(format: Format, bytes: &[u8]) -> Result<Self, String> {
        let zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "This Office document could not be opened.")?;
        if zip.len() > 2048 {
            return Err("This archive contains more than 2,048 entries.".into());
        }
        let mut file = Package { zip, read: 0, limited: false };
        let document = match format {
            Format::Word => DocumentView::Word(word(&file.part("word/document.xml")?, &mut file.limited)?),
            Format::Slides => {
                let source = file.part("ppt/presentation.xml")?;
                let doc = xml(&source)?;
                let parts = file.related_parts("ppt", "presentation.xml")?;
                let mut slides = Vec::new();
                for id in doc.root_element().descendants().filter(|n| tag(*n, "sldId")) {
                    if slides.len() == MAX_PAGES { file.limited = true; break }
                    let path = relationship_id(id).and_then(|id| parts.get(id)).filter(|path| path.starts_with("ppt/slides/"))
                        .ok_or("A slide is missing from this presentation.")?;
                    slides.push(slide(&file.part(path)?, &mut file.limited)?);
                }
                if slides.is_empty() { return Err("This presentation has no slides.".into()) }
                DocumentView::Slides(slides)
            }
            Format::Sheet => {
                let source = file.part("xl/workbook.xml")?;
                let doc = xml(&source)?;
                let parts = file.related_parts("xl", "workbook.xml")?;
                let shared = if file.zip.index_for_name("xl/sharedStrings.xml").is_some() {
                    let source = file.part("xl/sharedStrings.xml")?;
                    xml(&source)?.root_element().children().filter(|n| tag(*n, "si")).map(runs).collect()
                } else { Vec::new() };
                let mut sheets = Vec::new();
                for node in doc.root_element().descendants().filter(|n| tag(*n, "sheet")) {
                    if matches!(node.attribute("state"), Some("hidden" | "veryHidden")) { continue }
                    if sheets.len() == 20 { file.limited = true; break }
                    let path = relationship_id(node).and_then(|id| parts.get(id)).filter(|path| path.starts_with("xl/worksheets/"))
                        .ok_or("A worksheet is missing from this workbook.")?;
                    let name = node.attribute("name").unwrap_or("Sheet").to_owned();
                    sheets.push(worksheet(&file.part(path)?, name, &shared, &mut file.limited)?);
                }
                if sheets.is_empty() { return Err("This workbook has no visible sheets.".into()) }
                DocumentView::Sheet(sheets)
            }
        };
        Ok(Self { document, active: 0, limited: file.limited })
    }

    pub(super) fn view(&self, t: &T) -> El {
        let (details, body) = match &self.document {
            DocumentView::Word(page) => ("1 page · Fit to width".to_owned(), page_view(page, false)),
            DocumentView::Slides(pages) => {
                let current = self.active;
                let total = pages.len();
                let navigation = super::bar(t)
                    .child(super::action("Previous slide", &icons::CHEVRON_LEFT, current == 0, t)
                        .on_press(move |app: &mut App| select(app, current.saturating_sub(1))))
                    .child(row().grow(1.0).justify_center().child(super::note(format!("{} / {total}", current + 1), t)))
                    .child(super::action("Next slide", &icons::CHEVRON_RIGHT, current + 1 >= total, t)
                        .on_press(move |app: &mut App| select(app, current + 1)));
                (format!("{total} slides · Fit to width"), col().grow(1.0).min_h(0.0)
                    .child(navigation).child(super::divider(t)).child(page_view(&pages[current], true)))
            }
            DocumentView::Sheet(sheets) => {
                let current = self.active;
                let sheet = &sheets[current];
                let tabs = row().h(36.0).px(8.0).gap(4.0).center_y().scroll_x().role("tablist").label("Sheets")
                    .child(row().gap(4.0).children(sheets.iter().enumerate().map(|(index, sheet)| {
                        kit::Button::new(sheet.name.clone()).size("xs").variant(if current == index { "secondary" } else { "ghost-muted" })
                            .on_press(move |app: &mut App| select(app, index)).build(t)
                    })));
                (format!("{} sheets", sheets.len()), col().grow(1.0).min_h(0.0)
                    .child(sheet_view(sheet, current, t)).child(super::divider(t)).child(tabs))
            }
        };
        col().grow(1.0).min_h(0.0).min_w(0.0)
            .child(super::bar(t).child(super::note(details, t)).role("status"))
            .child(super::divider(t))
            .children(self.limited.then(|| super::note("Preview limited. Save it or open it in another app to read the full document.", t).wrap().padding(8.0)))
            .child(body)
    }
}

fn select(app: &mut App, index: usize) {
    let Some(super::Content::Office(office)) = super::active_mut(app) else { return };
    let len = match &office.document {
        DocumentView::Word(_) => 1,
        DocumentView::Sheet(sheets) => sheets.len(),
        DocumentView::Slides(pages) => pages.len(),
    };
    office.active = index.min(len - 1);
}

fn page_view(page: &Page, slide: bool) -> El {
    let ink = rune::Color::BLACK;
    let mut paper = col().w_full().max_w(816.0).min_w(0.0).min_h(if slide { 300.0 } else { 640.0 })
        .padding(if slide { 28.0 } else { 36.0 }).gap(12.0).fill(rune::Color::WHITE);
    for block in &page.blocks {
        paper = paper.child(match block {
            Block::Paragraph { text: value, heading } => text(value.clone())
                .font(if *heading { theme::weight(theme::TEXT_LG, 600.0) } else { theme::TEXT_SM })
                .color(ink).wrap().selectable(true),
            Block::Table(rows) => col().children(rows.iter().map(|values| {
                row().w_full().children(values.iter().map(|value| row().grow(1.0).min_w(0.0).padding(6.0)
                    .border(1.0, ink.alpha(0.22))
                    .child(text(value.clone()).font(theme::TEXT_XS).color(ink).wrap_anywhere().selectable(true))))
            })),
        });
    }
    col().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false })
        .child(row().w_full().padding(16.0).justify_center().child(paper))
}

fn column_label(column: usize) -> String {
    if column < 26 { ((b'A' + column as u8) as char).to_string() }
    else { format!("A{}", (b'A' + (column - 26) as u8) as char) }
}

fn sheet_view(sheet: &Sheet, current: usize, t: &T) -> El {
    let cell = |value: &str, heading| {
        row().w(112.0).h(24.0).px(6.0).center_y().border(1.0, t.c.border)
            .child(text(value.to_owned()).font(if heading { theme::XS_MEDIUM } else { theme::TEXT_XS })
                .color(t.c.foreground).truncate().selectable(!heading))
    };
    let heading = row().fill(t.c.background).sticky_top(0.0).child(table_number(None, t))
        .children((0..sheet.columns).map(|column| cell(&column_label(column), true)));
    let grid = col().child(heading).children(sheet.rows.iter().enumerate().map(|(i, values)| {
        row().child(table_number(Some(i + 1), t))
            .children((0..sheet.columns).map(|column| cell(values.get(column).map_or("", String::as_str), false)))
    }));
    el().key(format!("office-worksheet-{current}")).label(sheet.name.clone()).grow(1.0)
        .min_h(0.0).min_w(0.0).scroll_xy().child(grid)
}

pub(super) fn fixture(format: Format) -> Result<Office, String> {
    use std::io::Write;
    let word = [("word/document.xml", r#"<w:document xmlns:w="urn:w"><w:body>
        <w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Release checklist</w:t></w:r></w:p>
        <w:p><w:r><w:t>Build and verify the release before publishing.</w:t></w:r></w:p>
        <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Linux</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Passed</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
    </w:body></w:document>"#)];
    let sheets = [
        ("xl/workbook.xml", r#"<workbook xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Results" r:id="r1"/><sheet name="Archive" r:id="r2"/></sheets></workbook>"#),
        ("xl/_rels/workbook.xml.rels", r#"<Relationships><Relationship Id="r1" Target="worksheets/results.xml"/><Relationship Id="r2" Target="/xl/worksheets/archive.xml"/></Relationships>"#),
        ("xl/sharedStrings.xml", r#"<sst><si><t>Machine</t></si><si><r><t>Build </t></r><r><t>box</t></r></si></sst>"#),
        ("xl/worksheets/results.xml", r#"<worksheet><sheetData>
            <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="inlineStr"><is><t>Tests</t></is></c><c r="C1" t="inlineStr"><is><t>Passed</t></is></c></row>
            <row r="2"><c r="A2" t="s"><v>1</v></c><c r="C2" t="b"><v>1</v></c></row>
            <row r="4"><c r="A4" t="inlineStr"><is><t>Studio</t></is></c><c r="B4"><f>24+6</f><v>30</v></c><c r="C4" t="b"><v>1</v></c></row>
        </sheetData></worksheet>"#),
        ("xl/worksheets/archive.xml", r#"<worksheet><sheetData/></worksheet>"#),
    ];
    let slides = [
        ("ppt/presentation.xml", r#"<p:presentation xmlns:p="urn:p" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="3" r:id="first"/><p:sldId id="4" r:id="second"/></p:sldIdLst></p:presentation>"#),
        ("ppt/_rels/presentation.xml.rels", r#"<Relationships><Relationship Id="second" Target="slides/slide1.xml"/><Relationship Id="first" Target="slides/slide2.xml"/></Relationships>"#),
        ("ppt/slides/slide1.xml", r#"<p:sld xmlns:p="urn:p" xmlns:a="urn:a"><p:sp><a:p><a:r><a:t>Next steps</a:t></a:r></a:p></p:sp></p:sld>"#),
        ("ppt/slides/slide2.xml", r#"<p:sld xmlns:p="urn:p" xmlns:a="urn:a"><p:sp><p:nvSpPr><p:ph type="title"/></p:nvSpPr><a:p><a:r><a:t>Release overview</a:t></a:r></a:p></p:sp>
        <p:sp><a:p><a:r><a:t>All build checks passed.</a:t></a:r></a:p></p:sp>
        <a:tbl><a:tr><a:tc><a:p><a:r><a:t>Linux</a:t></a:r></a:p></a:tc><a:tc><a:p><a:r><a:t>Ready</a:t></a:r></a:p></a:tc></a:tr></a:tbl>
        </p:sld>"#),
    ];
    let parts: &[(&str, &str)] = match format {
        Format::Word => &word,
        Format::Sheet => &sheets,
        Format::Slides => &slides,
    };
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, xml) in parts {
        zip.start_file(name, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).map_err(|err| err.to_string())?;
        zip.write_all(xml.as_bytes()).map_err(|err| err.to_string())?;
    }
    let bytes = zip.finish().map_err(|err| err.to_string())?.into_inner();
    Office::open(format, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_word_blocks_and_preserves_slide_and_sheet_order() {
        let DocumentView::Word(page) = fixture(Format::Word).unwrap().document else { panic!("Word document") };
        assert!(matches!(&page.blocks[0], Block::Paragraph { text, heading: true } if text == "Release checklist"));
        assert!(matches!(&page.blocks[2], Block::Table(rows) if rows[0] == ["Linux", "Passed"]));

        let DocumentView::Slides(slides) = fixture(Format::Slides).unwrap().document else { panic!("presentation") };
        assert_eq!(slides.len(), 2);
        assert!(matches!(&slides[0].blocks[0], Block::Paragraph { text, heading: true } if text == "Release overview"));
        assert!(matches!(&slides[1].blocks[0], Block::Paragraph { text, heading: false } if text == "Next steps"));

        let DocumentView::Sheet(sheets) = fixture(Format::Sheet).unwrap().document else { panic!("workbook") };
        assert_eq!(sheets.iter().map(|sheet| sheet.name.as_str()).collect::<Vec<_>>(), ["Results", "Archive"]);
        assert_eq!(sheets[0].rows[1], ["Build box", "", "TRUE"]);
        assert!(sheets[0].rows[2].is_empty(), "a sparse row stays in its original position");
        assert_eq!(sheets[0].rows[3], ["Studio", "30", "TRUE"]);
        assert!(relation_path("xl", "../outside.xml").is_none());
    }
}
