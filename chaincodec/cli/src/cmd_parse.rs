//! `chaincodec parse` — validate and pretty-print a CSDL file.

use anyhow::Result;
use chaincodec_registry::CsdlParser;

pub fn run(file: &str) -> Result<()> {
    let content = std::fs::read_to_string(file)?;
    match CsdlParser::parse(&content) {
        Ok(schema) => {
            println!("✓ Schema '{}' v{} parsed successfully", schema.name, schema.version);
            println!("  Chains:      {}", schema.chains.join(", "));
            println!("  Event:       {}", schema.event);
            println!("  Fingerprint: {}", schema.fingerprint);
            println!("  Fields:      {}", schema.fields.len());
            for (name, field) in &schema.fields {
                let indexed = if field.indexed { " [indexed]" } else { "" };
                println!("    - {}: {}{}", name, field.ty, indexed);
            }
            println!("  Trust level: {}", schema.meta.trust_level);
        }
        Err(e) => {
            eprintln!("✗ Parse error: {e}");
            std::process::exit(1);
        }
    }
    Ok(())
}
