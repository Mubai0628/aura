use super::*;

pub(super) fn pretty_fixture_json(value: &impl Serialize) -> Result<Vec<u8>> {
    let value = serde_json::to_value(value)?;
    let mut output = String::new();
    write_fixture_json_value(&mut output, &value, 0)?;
    output.push('\n');
    Ok(output.into_bytes())
}

fn write_fixture_json_value(output: &mut String, value: &Value, indent: usize) -> Result<()> {
    match value {
        Value::Object(object) if object.is_empty() => output.push_str("{}"),
        Value::Object(object) => {
            output.push_str("{\n");
            for (index, (key, child)) in object.iter().enumerate() {
                output.push_str(&" ".repeat(indent + 2));
                output.push_str(&serde_json::to_string(key)?);
                output.push_str(": ");
                write_fixture_json_value(output, child, indent + 2)?;
                if index + 1 != object.len() {
                    output.push(',');
                }
                output.push('\n');
            }
            output.push_str(&" ".repeat(indent));
            output.push('}');
        }
        Value::Array(array) if array.is_empty() => output.push_str("[]"),
        Value::Array(array) => {
            let inline = array
                .iter()
                .all(|child| !matches!(child, Value::Array(_) | Value::Object(_)))
                .then(|| {
                    array
                        .iter()
                        .map(serde_json::to_string)
                        .collect::<std::result::Result<Vec<_>, _>>()
                        .map(|items| format!("[{}]", items.join(", ")))
                })
                .transpose()?;
            let current_column = output.rsplit('\n').next().map_or(0, |line| line.chars().count());
            if let Some(inline) =
                inline.filter(|inline| current_column + inline.chars().count() <= 80)
            {
                output.push_str(&inline);
            } else {
                output.push_str("[\n");
                for (index, child) in array.iter().enumerate() {
                    output.push_str(&" ".repeat(indent + 2));
                    write_fixture_json_value(output, child, indent + 2)?;
                    if index + 1 != array.len() {
                        output.push(',');
                    }
                    output.push('\n');
                }
                output.push_str(&" ".repeat(indent));
                output.push(']');
            }
        }
        _ => output.push_str(&serde_json::to_string(value)?),
    }
    Ok(())
}
