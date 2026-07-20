//! Parse funnel payloads as JS value literals (oxc), not hand-rolled JSON.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    ArrayExpressionElement, Expression, ObjectPropertyKind, PropertyKind, UnaryOperator,
};
use oxc_parser::Parser;
use oxc_span::SourceType;
use serde_json::{Map, Number, Value};

/// Parse a funnel file / expression as a static JS value literal via oxc.
///
/// Accepts object/array/string/number/boolean/`null` literals (JSON is valid JS).
/// Rejects identifiers, calls, spreads, methods, holes, and other non-literal forms.
pub fn parse_js_value(source: &str) -> std::result::Result<Value, String> {
    let source = source.trim();
    if source.is_empty() {
        return Err("empty JS value".into());
    }

    let allocator = Allocator::default();
    let expr = Parser::new(&allocator, source, SourceType::mjs())
        .parse_expression()
        .map_err(|_| "not a valid JS expression".to_string())?;

    expr_to_value(&expr)
}

fn expr_to_value(expr: &Expression<'_>) -> std::result::Result<Value, String> {
    match expr {
        Expression::NullLiteral(_) => Ok(Value::Null),
        Expression::BooleanLiteral(b) => Ok(Value::Bool(b.value)),
        Expression::StringLiteral(s) => Ok(Value::String(s.value.as_str().to_string())),
        Expression::NumericLiteral(n) => number_value(n.value),
        Expression::UnaryExpression(u)
            if u.operator == UnaryOperator::UnaryNegation
                || u.operator == UnaryOperator::UnaryPlus =>
        {
            match &u.argument {
                Expression::NumericLiteral(n) => {
                    let v = if u.operator == UnaryOperator::UnaryNegation {
                        -n.value
                    } else {
                        n.value
                    };
                    number_value(v)
                }
                _ => Err("unary +/- only allowed on numeric literals".into()),
            }
        }
        Expression::ParenthesizedExpression(p) => expr_to_value(&p.expression),
        Expression::ArrayExpression(arr) => {
            let mut out = Vec::with_capacity(arr.elements.len());
            for el in &arr.elements {
                match el {
                    ArrayExpressionElement::SpreadElement(_) => {
                        return Err("spreads are not supported in funnel values".into());
                    }
                    ArrayExpressionElement::Elision(_) => {
                        return Err("array holes are not supported in funnel values".into());
                    }
                    other => {
                        let Some(expr) = other.as_expression() else {
                            return Err("unsupported array element in funnel value".into());
                        };
                        out.push(expr_to_value(expr)?);
                    }
                }
            }
            Ok(Value::Array(out))
        }
        Expression::ObjectExpression(obj) => {
            let mut map = Map::new();
            for prop in &obj.properties {
                let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                    return Err("spreads are not supported in funnel values".into());
                };
                if prop.method || prop.shorthand || prop.kind != PropertyKind::Init || prop.computed
                {
                    return Err(
                        "only plain `key: value` properties are supported in funnel values".into(),
                    );
                }
                let key = prop
                    .key
                    .static_name()
                    .ok_or_else(|| {
                        "only identifier or string/number literal keys are supported".to_string()
                    })?
                    .into_owned();
                let value = expr_to_value(&prop.value)?;
                map.insert(key, value);
            }
            Ok(Value::Object(map))
        }
        Expression::TemplateLiteral(t) if t.expressions.is_empty() => {
            let mut s = String::new();
            for q in &t.quasis {
                let cooked = q
                    .value
                    .cooked
                    .as_ref()
                    .ok_or_else(|| "invalid template literal escape in funnel value".to_string())?;
                s.push_str(cooked.as_str());
            }
            Ok(Value::String(s))
        }
        _ => Err(
            "funnel values must be static JS literals (object, array, string, number, boolean, null)"
                .into(),
        ),
    }
}

fn number_value(v: f64) -> std::result::Result<Value, String> {
    Number::from_f64(v)
        .map(Value::Number)
        .ok_or_else(|| format!("invalid number literal `{v}`"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_json_compatible_and_js_literals() {
        assert_eq!(parse_js_value("null").unwrap(), Value::Null);
        assert_eq!(parse_js_value("true").unwrap(), json!(true));
        assert_eq!(parse_js_value("42").unwrap(), json!(42.0));
        assert_eq!(parse_js_value("-3").unwrap(), json!(-3.0));
        assert_eq!(parse_js_value(r#""hi""#).unwrap(), json!("hi"));
        assert_eq!(parse_js_value("'hi'").unwrap(), json!("hi"));
        assert_eq!(
            parse_js_value(r#"[{ slug: "a", n: 1, }]"#).unwrap(),
            json!([{ "slug": "a", "n": 1.0 }])
        );
        assert_eq!(
            parse_js_value(r#"{"slug":"a"}"#).unwrap(),
            json!({"slug": "a"})
        );
    }

    #[test]
    fn rejects_non_literals() {
        assert!(parse_js_value("foo").is_err());
        assert!(parse_js_value("{ foo }").is_err());
        assert!(parse_js_value("[...a]").is_err());
        assert!(parse_js_value("1 + 1").is_err());
        assert!(parse_js_value("").is_err());
    }
}
