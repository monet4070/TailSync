use std::collections::BTreeSet;

use tailsync_core::crypto::Settings;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Keep the public Settings contract on JSON Schema Draft 7. Schemars 1.x
    // defaults to Draft 2020-12, which would rename `definitions` to `$defs`
    // and silently change the schema consumed by the desktop generators.
    let schema = schemars::generate::SchemaSettings::draft07()
        .into_generator()
        .into_root_schema_for::<Settings>();
    let mut document = serde_json::to_value(schema)?;
    let object = document
        .as_object_mut()
        .ok_or("Settings schema root is not an object")?;
    object.insert(
        "$id".to_string(),
        serde_json::json!("https://tailsync.dev/schema/settings-v2.json"),
    );
    object.insert("title".to_string(), serde_json::json!("TailSync Settings"));
    object.insert("additionalProperties".to_string(), serde_json::json!(false));

    let defaults = serde_json::to_value(Settings::default())?;
    let defaults = defaults
        .as_object()
        .ok_or("Settings defaults are not an object")?;
    let properties = object
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("Settings schema has no properties")?;
    let required = properties.keys().cloned().collect::<BTreeSet<_>>();
    for (name, definition) in properties.iter_mut() {
        let default = defaults
            .get(name)
            .ok_or_else(|| format!("Settings default is missing {name}"))?;
        definition
            .as_object_mut()
            .ok_or_else(|| format!("Settings property {name} is not an object"))?
            .insert("default".to_string(), default.clone());
    }
    object.insert("required".to_string(), serde_json::to_value(required)?);

    println!("{}", serde_json::to_string_pretty(&document)?);
    Ok(())
}
