use ariadne::{Color, Label, Report, ReportKind, Source};

use crate::elaborate::SpannedError;
use crate::parser::ParseError;

pub fn report_parse_errors(source: &str, filename: &str, errors: &[ParseError]) {
    for error in errors {
        Report::build(ReportKind::Error, (filename, error.span.0..error.span.1))
            .with_message(&error.message)
            .with_label(
                Label::new((filename, error.span.0..error.span.1))
                    .with_message(&error.message)
                    .with_color(Color::Red),
            )
            .finish()
            .print((filename, Source::from(source)))
            .unwrap();
    }
}

pub fn report_elab_error(source: &str, filename: &str, error: &SpannedError) {
    let msg = format!("{}", error.error);
    match error.span {
        Some(span) => {
            Report::build(ReportKind::Error, (filename, span.0..span.1))
                .with_message(&msg)
                .with_label(
                    Label::new((filename, span.0..span.1))
                        .with_message(&msg)
                        .with_color(Color::Red),
                )
                .finish()
                .print((filename, Source::from(source)))
                .unwrap();
        }
        None => {
            eprintln!("Error: {msg}");
        }
    }
}
