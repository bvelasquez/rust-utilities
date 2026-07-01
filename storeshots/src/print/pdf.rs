use image::RgbaImage;
use printpdf::{
    Op, PdfDocument, PdfPage, PdfSaveOptions, RawImage, XObjectTransform, Mm,
};
use std::path::Path;

pub struct PdfPageSpec {
    pub image: RgbaImage,
    pub width_in: f64,
    pub height_in: f64,
}

pub fn write_pdf(path: &Path, title: &str, pages: &[PdfPageSpec]) -> anyhow::Result<()> {
    if pages.is_empty() {
        anyhow::bail!("no pages for PDF");
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut doc = PdfDocument::new(title);
    let pdf_pages: Vec<PdfPage> = pages
        .iter()
        .map(|page| page_to_pdf_page(&mut doc, page))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let bytes = doc
        .with_pages(pdf_pages)
        .save(&PdfSaveOptions::default(), &mut Vec::new());
    std::fs::write(path, bytes)?;
    Ok(())
}

fn page_to_pdf_page(doc: &mut PdfDocument, page: &PdfPageSpec) -> anyhow::Result<PdfPage> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    let dynamic = image::DynamicImage::ImageRgba8(page.image.clone());
    dynamic.write_to(&mut buffer, image::ImageFormat::Png)?;
    let raw = RawImage::decode_from_bytes(buffer.get_ref(), &mut Vec::new())
        .map_err(|e| anyhow::anyhow!("decode PNG for PDF: {e}"))?;
    let image_id = doc.add_image(&raw);
    let dpi = page.image.width() as f32 / page.width_in as f32;
    let ops = vec![Op::UseXobject {
        id: image_id,
        transform: XObjectTransform {
            dpi: Some(dpi),
            ..Default::default()
        },
    }];
    Ok(PdfPage::new(
        Mm(inches_to_mm(page.width_in)),
        Mm(inches_to_mm(page.height_in)),
        ops,
    ))
}

fn inches_to_mm(inches: f64) -> f32 {
    (inches * 25.4) as f32
}
