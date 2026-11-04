//! Lightweight language heuristics for rule matching (no NLP crates).

use crate::rules::patterns::header_value;
use crate::store::CachedMessage;

/// True when the message looks primarily Spanish (headers + subject/body cues).
pub fn message_is_spanish(msg: &CachedMessage) -> bool {
    let content_language = header_value(msg, "Content-Language");
    is_spanish(
        &msg.subject,
        &msg.body_preview,
        content_language.as_deref(),
    )
}

pub fn is_spanish(subject: &str, body: &str, content_language: Option<&str>) -> bool {
    if content_language_is_spanish(content_language) {
        return true;
    }
    let text = format!("{subject}\n{body}");
    spanish_score(&text) >= 3
}

fn content_language_is_spanish(cl: Option<&str>) -> bool {
    let Some(cl) = cl else {
        return false;
    };
    cl.split(',').any(|part| {
        let tag = part
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        tag == "es" || tag.starts_with("es-")
    })
}

fn spanish_score(text: &str) -> i32 {
    let lower = text.to_lowercase();
    let mut score = 0i32;

    // Orthography almost never appears in English mail.
    if text.contains('¿') {
        score += 3;
    }
    if text.contains('¡') {
        score += 3;
    }
    if lower.contains('ñ') {
        score += 2;
    }

    // Distinctive Spanish phrases / marketing spam vocabulary.
    const WORDS: &[&str] = &[
        "usted",
        "ustedes",
        "gracias",
        "oferta",
        "descuento",
        "información",
        "informacion",
        "disponible",
        "envío",
        "envio",
        "pedido",
        "contraseña",
        "contrasena",
        "verificación",
        "verificacion",
        "suscripción",
        "suscripcion",
        "también",
        "tambien",
        "exclusivo",
        "felicidades",
        "estimado",
        "estimada",
        "bienvenido",
        "bienvenida",
        "después",
        "despues",
        "para ti",
        "para usted",
        "más información",
        "mas informacion",
        "correo electrónico",
        "correo electronico",
        "no responda",
        "no responder",
        "darse de baja",
        "cancelar suscripción",
        "cancelar suscripcion",
        "tu pedido",
        "su pedido",
        "tu cuenta",
        "su cuenta",
        "activa tu",
        "activa su",
        "novedades",
        "promoción",
        "promocion",
        "oportunidad",
        "hola,",
        "¡hola",
        "¿quieres",
        "¿cómo",
        "cómo estás",
        "como estas",
        "últimas",
        "ultimas",
    ];
    for w in WORDS {
        if lower.contains(w) {
            score += 1;
        }
    }

    // Function-word phrases that co-occur in Spanish prose.
    const PHRASES: &[&str] = &[
        " de la ",
        " de los ",
        " de las ",
        " en el ",
        " en la ",
        " para el ",
        " para la ",
        " con el ",
        " con la ",
        " por el ",
        " por la ",
        " a la ",
        " del ",
        " que ",
    ];
    let phrase_hits = PHRASES.iter().filter(|p| lower.contains(*p)).count();
    if phrase_hits >= 3 {
        score += 2;
    } else if phrase_hits >= 2 {
        score += 1;
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_language_header() {
        assert!(is_spanish("Hello", "body", Some("es-MX")));
        assert!(is_spanish("Hello", "body", Some("es")));
        assert!(!is_spanish("Hello", "body", Some("en-US")));
    }

    #[test]
    fn spanish_marketing_subject() {
        assert!(is_spanish(
            "¡Oferta exclusiva para ti!",
            "Aprovecha este descuento hoy",
            None
        ));
    }

    #[test]
    fn english_stays_clean() {
        assert!(!is_spanish(
            "Your order has shipped",
            "Track your package at amazon.com",
            None
        ));
        assert!(!is_spanish(
            "Weekly digest from El Capitan Club",
            "See this week's de facto picks",
            None
        ));
    }

    #[test]
    fn inverted_question_is_enough() {
        assert!(is_spanish("¿Listo para ahorrar?", "Click here", None));
    }
}
