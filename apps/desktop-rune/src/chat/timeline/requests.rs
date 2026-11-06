use super::rows::array;
use crate::data::s;
use serde_json::{Map, Value};
use std::collections::HashMap;

pub fn answers(
    item: &Value,
    selected: &HashMap<String, Vec<String>>,
    custom: impl Fn(&str) -> Option<String>,
) -> Value {
    let mut result = Map::new();
    for question in array(item, "questions") {
        let id = s(question, "id");
        if question["allowCustomAnswer"].as_bool() != Some(false) {
            if let Some(answer) = custom(id).filter(|a| !a.trim().is_empty()) {
                result.insert(id.to_owned(), Value::String(answer.trim().to_owned()));
                continue;
            }
        }
        if let Some(values) = selected.get(id).filter(|v| !v.is_empty()) {
            result.insert(
                id.to_owned(),
                if question["multiSelect"].as_bool() == Some(true) {
                    serde_json::json!(values)
                } else {
                    serde_json::json!(values[0])
                },
            );
        }
    }
    Value::Object(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn single_answers_are_strings_multiple_answers_are_arrays_and_custom_takes_precedence() {
        let item = json!({"questions":[{"id":"single"},{"id":"multi","multiSelect":true},{"id":"custom"},{"id":"fixed","allowCustomAnswer":false}]});
        let selected = HashMap::from([
            ("single".to_owned(), vec!["one".to_owned()]),
            ("multi".to_owned(), vec!["one".to_owned(), "two".to_owned()]),
            ("custom".to_owned(), vec!["option".to_owned()]),
            ("fixed".to_owned(), vec!["option".to_owned()]),
        ]);
        let answers = answers(&item, &selected, |id| {
            matches!(id, "custom" | "fixed").then(|| "  typed  ".to_owned())
        });
        assert_eq!(
            answers,
            json!({"single":"one","multi":["one","two"],"custom":"typed","fixed":"option"})
        );
    }
}
