//! Image extraction from small PDFs built in memory.

use lopdf::{Document, Object, Stream, dictionary};

/// A two-page PDF showing the same 4×2 gray image, upright on page 1 and
/// drawn with a vertically flipped matrix on page 2. MediaBox is inherited.
fn two_page_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let image = Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 4,
            "Height" => 2,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
        },
        vec![0, 64, 128, 255, 200, 200, 200, 200],
    );
    let image_id = doc.add_object(image);

    let mut kids = Vec::new();
    for content in [
        "q 100 0 0 50 72 600 cm /Im0 Do Q",
        "q 100 0 0 -50 72 650 cm /Im0 Do Q",
    ] {
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
        });
        kids.push(Object::Reference(page_id));
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => 2,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("save PDF");
    bytes
}

#[test]
fn images_are_placed_and_oriented() {
    let doc = scorelift_pdf::load_mem(&two_page_pdf()).expect("load PDF");
    assert_eq!(doc.pages.len(), 2);

    for page in &doc.pages {
        assert_eq!((page.width, page.height), (612.0, 792.0));
        assert_eq!(page.images.len(), 1, "page {}", page.number);
        let img = &page.images[0];
        assert_eq!(img.name, "Im0");
        let r = img.bbox;
        assert_eq!((r.x, r.y, r.w, r.h), (72.0, 142.0, 100.0, 50.0));
        assert!((img.dpi() - 2.88).abs() < 0.01);
    }

    let upright = &doc.pages[0].images[0].image;
    let row0: Vec<u8> = (0..4).map(|x| upright.get_pixel(x, 0).0[0]).collect();
    assert_eq!(row0, [0, 64, 128, 255]);

    // The flipped matrix puts the image's last row on top.
    let flipped = &doc.pages[1].images[0].image;
    let row0: Vec<u8> = (0..4).map(|x| flipped.get_pixel(x, 0).0[0]).collect();
    assert_eq!(row0, [200, 200, 200, 200]);
}

#[test]
fn garbage_is_an_error() {
    assert!(scorelift_pdf::load_mem(b"not a pdf").is_err());
}
