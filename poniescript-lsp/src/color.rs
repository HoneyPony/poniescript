use std::collections::HashMap;

use poniescript_core::lexer::Token;
use tower_lsp::lsp_types::*;

use poniescript_core::{
    db::*,
    expr::*,
    source::*,
};

use crate::document::DocumentStore;
use crate::document::*;

pub fn convert_color(db: &Db, token: &Token) -> Color {
    let subslice = {
        let string = db.get(token.lexeme);
        &string[2..string.len() - 1]
    };

    fn conv(x: char) -> u32 {
        match x {
            '0'..='9' => { x as u32 - '0' as u32 }
            'a'..='f' => { x as u32 - 'a' as u32 + 10 }
            'A'..='F' => { x as u32 - 'A' as u32 + 10 }
            _ => unreachable!("ICE: Bad color literal")
        }
    }

    let mut values: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
    let mut idx = 0;

    if subslice.len() <= 4 {
        for c in subslice.chars() {
            let value = conv(c);
            let value = value * 16 + value;
            let value = value as f32 / 255.0;

            values[idx] = value;
            idx += 1;
        }
    }
    else {
        let mut on_even = false;
        let mut current: u32 = 0;
        for c in subslice.chars() {
            let value = conv(c);
            current = current * 16 + value;
            
            if on_even {
                // IMPORTANT: Use 'current' here, not 'value'.
                let value = current as f32 / 255.0;

                values[idx] = value;
                idx += 1;

                current = 0;
            }

            on_even = !on_even;
        }
    }

    Color {
        red:   values[0],
        green: values[1],
        blue:  values[2],
        alpha: values[3]
    }
}