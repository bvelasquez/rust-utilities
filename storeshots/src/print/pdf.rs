use image::RgbaImage;
use printpdf::{
    ImageCompression, ImageOptimizationOptions, Op, PdfDocument, PdfPage, PdfSaveOptions,
    PdfWarnMsg, RawImage, RawImageData, RawImageFormat, XObjectTransform, Mm,
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

    let mut warnings = Vec::new();
    let bytes = doc
        .with_pages(pdf_pages)
        .save(&print_save_options(), &mut warnings);
    if !warnings.is_empty() {
        for w in &warnings {
            eprintln!("warning: PDF save: {w:?}");
        }
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Print-quality save — do not downscale raster pages (printpdf default caps at 2MB/image).
fn print_save_options() -> PdfSaveOptions {
    PdfSaveOptions {
        optimize: true,
        subset_fonts: true,
        secure: true,
        image_optimization: Some(ImageOptimizationOptions {
            max_image_size: None,
            quality: Some(0.95),
            auto_optimize: Some(true),
            convert_to_greyscale: Some(false),
            dither_greyscale: None,
            format: Some(ImageCompression::Flate),
        }),
    }
}

fn page_to_pdf_page(doc: &mut PdfDocument, page: &PdfPageSpec) -> anyhow::Result<PdfPage> {
    let raw = rgba_to_raw(&page.image);
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

fn rgba_to_raw(image: &RgbaImage) -> RawImage {
    RawImage {
        pixels: RawImageData::U8(image.as_raw().to_vec()),
        width: image.width() as usize,
        height: image.height() as usize,
        data_format: RawImageFormat::RGBA8,
        tag: Vec::new(),
    }
}

fn inches_to_mm(inches: f64) -> f32 {
    (inches * 25.4) as f32
}
