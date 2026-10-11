use std::path::PathBuf;
use tailsync_core::composite::{
    packed_to_png,
    restore::{RestoreInput, RestorePlan, RestoreTarget},
    Completeness, Mime, PortableBundle, Representation, Resource, ResourceBinding,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(std::env::args().nth(1).ok_or("output directory required")?);
    if !output.is_absolute() || output.exists() {
        return Err("an unused absolute output directory is required".into());
    }
    let html = "<link rel='stylesheet' href='https://fixture.invalid/site.css'><p><b>中文 🙂</b><i> italic</i></p><img src='tailsync-resource:1'><p>after picture</p><img src='https://fixture.invalid/missing'>";
    let bundle = PortableBundle {
        representations: vec![Representation {
            mime: Mime::Html,
            bytes: html.as_bytes().to_vec(),
        }],
        resources: vec![Resource {
            id: 1,
            png: packed_to_png(16, 16, &[255, 0, 0, 255].repeat(256))?,
        }],
        bindings: vec![ResourceBinding {
            representation: 0,
            resource: 1,
            offset: html
                .find("tailsync-resource:1")
                .ok_or("fixture reference missing")? as u32,
            length: "tailsync-resource:1".len() as u32,
        }],
        completeness: Completeness::ExternalReferences,
    };
    let before = bundle.encode()?;
    let plan = RestorePlan::available_content(
        &bundle,
        RestoreInput {
            source_platform: None,
            requested_format: None,
        },
        RestoreTarget::Macos,
    )?;
    if plan.missing_images() != 1 || bundle.encode()? != before {
        return Err("fixture preparation changed history or missing-image count".into());
    }
    std::fs::create_dir(&output)?;
    let snapshot = plan.native().ok_or("native output required")?;
    for (name, native) in [
        ("document.rtf", "public.rtf"),
        ("document.html", "public.html"),
        ("document.txt", "public.utf8-plain-text"),
    ] {
        let format = snapshot
            .formats
            .iter()
            .find(|format| format.name == native)
            .ok_or("prepared format missing")?;
        std::fs::write(output.join(name), &format.bytes)?;
    }
    println!("prepared text, HTML and RTF; missing_images=1; original_unchanged=true");
    Ok(())
}
