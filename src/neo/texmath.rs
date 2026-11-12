// LaTeX の数式（`$…$`・`$$…$$`）を、HelpView で読める文字にする（GUI なし。簡易対応）。
// 記号は Unicode の文字に、`^`・`_` は上付き・下付きの文字に（無い文字は `^(…)`）、`\frac{a}{b}` は `a/b` にする。
// 分数の積み重ね・行列・大きな括弧の形は出せない。知らない命令はそのまま `\名前` で残す。

/// 数式の LaTeX を文字にする。
pub fn to_text(tex: &str) -> String {
    let chars: Vec<char> = tex.chars().collect();
    let mut i = 0;
    let s = expr(&chars, &mut i, None);
    // 連続した空白を 1 つに
    let mut out = String::with_capacity(s.len());
    for c in s.trim().chars() {
        if c == ' ' && out.ends_with(' ') {
            continue;
        }
        out.push(c);
    }
    out
}

/// `stop` の文字（`}` など）か終わりまでを読む。
fn expr(c: &[char], i: &mut usize, stop: Option<char>) -> String {
    let mut out = String::new();
    while *i < c.len() {
        let ch = c[*i];
        if Some(ch) == stop {
            *i += 1;
            return out;
        }
        match ch {
            '{' => {
                *i += 1;
                out.push_str(&expr(c, i, Some('}')));
            }
            '}' => *i += 1, // 対応の無い閉じ
            '^' | '_' => {
                *i += 1;
                let arg = arg(c, i);
                out.push_str(&script(&arg, ch == '^'));
            }
            '\\' => {
                *i += 1;
                out.push_str(&command(c, i));
            }
            '~' => {
                *i += 1;
                out.push(' ');
            }
            '\n' | '\t' => {
                *i += 1;
                out.push(' ');
            }
            _ => {
                *i += 1;
                out.push(ch);
            }
        }
    }
    out
}

/// 命令の引数を 1 つ（`{…}`・`\命令`・1 文字）。空白は飛ばす。
fn arg(c: &[char], i: &mut usize) -> String {
    while *i < c.len() && c[*i] == ' ' {
        *i += 1;
    }
    match c.get(*i) {
        Some('{') => {
            *i += 1;
            expr(c, i, Some('}'))
        }
        Some('\\') => {
            *i += 1;
            command(c, i)
        }
        Some(&ch) => {
            *i += 1;
            ch.to_string()
        }
        None => String::new(),
    }
}

/// 1 文字より長い式は、分数などで括弧に入れる。
fn group(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= 1 || s.chars().all(|c| c.is_alphanumeric()) {
        s.to_string()
    } else {
        format!("({s})")
    }
}

fn script(s: &str, sup: bool) -> String {
    let s = s.trim();
    let mapped: Option<String> = s.chars().map(|c| if sup { sup_char(c) } else { sub_char(c) }).collect();
    match mapped {
        Some(m) if !m.is_empty() => m,
        _ if s.chars().count() <= 1 => format!("{}{s}", if sup { '^' } else { '_' }),
        _ => format!("{}({s})", if sup { '^' } else { '_' }),
    }
}

/// `\` の後ろを読んで文字にする。
fn command(c: &[char], i: &mut usize) -> String {
    let start = *i;
    while *i < c.len() && c[*i].is_ascii_alphabetic() {
        *i += 1;
    }
    if *i == start {
        // `\,` `\{` などの 1 文字の命令
        let Some(&ch) = c.get(*i) else { return String::new() };
        *i += 1;
        return match ch {
            ',' | ';' | ':' | '!' | ' ' => " ".into(),
            '\\' => " ".into(),
            _ => ch.to_string(),
        };
    }
    let name: String = c[start..*i].iter().collect();
    match name.as_str() {
        "frac" | "dfrac" | "tfrac" => {
            let a = arg(c, i);
            let b = arg(c, i);
            format!("{}/{}", group(&a), group(&b))
        }
        "sqrt" => {
            // \sqrt[n]{x}
            let mut n = String::new();
            if c.get(*i) == Some(&'[') {
                *i += 1;
                n = expr(c, i, Some(']'));
            }
            let x = arg(c, i);
            let root = match n.trim() {
                "" => "√".to_string(),
                "3" => "∛".to_string(),
                "4" => "∜".to_string(),
                n => format!("{}√", script(n, true)),
            };
            format!("{root}{}", group(&x))
        }
        "text" | "mathrm" | "mathbf" | "mathit" | "mathsf" | "mathtt" | "operatorname" | "textbf" | "textit" | "mbox" | "boldsymbol" => arg(c, i),
        "mathbb" => {
            let a = arg(c, i);
            a.chars().map(|ch| blackboard(ch).unwrap_or(ch)).collect()
        }
        "left" | "right" | "big" | "Big" | "bigg" | "Bigg" | "displaystyle" | "limits" | "nolimits" => String::new(),
        "quad" => "  ".into(),
        "qquad" => "    ".into(),
        "begin" | "end" => {
            arg(c, i); // 環境の名前は出さない
            " ".into()
        }
        "sin" | "cos" | "tan" | "log" | "ln" | "exp" | "lim" | "max" | "min" | "det" | "sup" | "inf" | "mod" | "gcd" => format!("{name} "),
        _ => match symbol(&name) {
            Some(s) => s.to_string(),
            None => format!("\\{name}"),
        },
    }
}

fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "α", "beta" => "β", "gamma" => "γ", "delta" => "δ", "epsilon" => "ε", "varepsilon" => "ε", "zeta" => "ζ",
        "eta" => "η", "theta" => "θ", "vartheta" => "ϑ", "iota" => "ι", "kappa" => "κ", "lambda" => "λ", "mu" => "μ", "nu" => "ν",
        "xi" => "ξ", "pi" => "π", "varpi" => "ϖ", "rho" => "ρ", "sigma" => "σ", "tau" => "τ", "upsilon" => "υ", "phi" => "φ",
        "varphi" => "φ", "chi" => "χ", "psi" => "ψ", "omega" => "ω",
        "Gamma" => "Γ", "Delta" => "Δ", "Theta" => "Θ", "Lambda" => "Λ", "Xi" => "Ξ", "Pi" => "Π", "Sigma" => "Σ", "Upsilon" => "Υ",
        "Phi" => "Φ", "Psi" => "Ψ", "Omega" => "Ω",
        "times" => "×", "cdot" => "·", "div" => "÷", "pm" => "±", "mp" => "∓", "ast" => "∗", "circ" => "∘", "bullet" => "•",
        "leq" | "le" => "≤", "geq" | "ge" => "≥", "neq" | "ne" => "≠", "approx" => "≈", "equiv" => "≡", "sim" => "∼", "simeq" => "≃",
        "propto" => "∝", "ll" => "≪", "gg" => "≫",
        "infty" => "∞", "partial" => "∂", "nabla" => "∇", "sum" => "∑", "prod" => "∏", "int" => "∫", "iint" => "∬", "oint" => "∮",
        "to" | "rightarrow" => "→", "leftarrow" | "gets" => "←", "leftrightarrow" => "↔", "Rightarrow" | "implies" => "⇒",
        "Leftarrow" => "⇐", "Leftrightarrow" | "iff" => "⇔", "mapsto" => "↦", "uparrow" => "↑", "downarrow" => "↓",
        "in" => "∈", "notin" => "∉", "ni" => "∋", "subset" => "⊂", "supset" => "⊃", "subseteq" => "⊆", "supseteq" => "⊇",
        "cup" => "∪", "cap" => "∩", "emptyset" | "varnothing" => "∅", "setminus" => "∖",
        "forall" => "∀", "exists" => "∃", "neg" | "lnot" => "¬", "land" | "wedge" => "∧", "lor" | "vee" => "∨", "oplus" => "⊕",
        "otimes" => "⊗", "perp" => "⊥", "parallel" => "∥", "angle" => "∠", "degree" => "°",
        "ldots" | "dots" => "…", "cdots" => "⋯", "vdots" => "⋮", "ddots" => "⋱",
        "langle" => "⟨", "rangle" => "⟩", "lfloor" => "⌊", "rfloor" => "⌋", "lceil" => "⌈", "rceil" => "⌉", "vert" | "mid" => "|",
        "Vert" => "‖", "hbar" => "ℏ", "ell" => "ℓ", "Re" => "ℜ", "Im" => "ℑ", "aleph" => "ℵ", "prime" => "′",
        "lbrace" => "{", "rbrace" => "}",
        _ => return None,
    })
}

fn sup_char(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰', '1' => '¹', '2' => '²', '3' => '³', '4' => '⁴', '5' => '⁵', '6' => '⁶', '7' => '⁷', '8' => '⁸', '9' => '⁹',
        '+' => '⁺', '-' => '⁻', '=' => '⁼', '(' => '⁽', ')' => '⁾', 'n' => 'ⁿ', 'i' => 'ⁱ', 'x' => 'ˣ', 'y' => 'ʸ', 'k' => 'ᵏ',
        'a' => 'ᵃ', 'b' => 'ᵇ', 'c' => 'ᶜ', 'd' => 'ᵈ', 'e' => 'ᵉ', 'm' => 'ᵐ', 't' => 'ᵗ', 'T' => 'ᵀ', '′' => '′', '*' => '*',
        _ => return None,
    })
}

fn sub_char(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀', '1' => '₁', '2' => '₂', '3' => '₃', '4' => '₄', '5' => '₅', '6' => '₆', '7' => '₇', '8' => '₈', '9' => '₉',
        '+' => '₊', '-' => '₋', '=' => '₌', '(' => '₍', ')' => '₎', 'a' => 'ₐ', 'e' => 'ₑ', 'i' => 'ᵢ', 'j' => 'ⱼ', 'k' => 'ₖ',
        'n' => 'ₙ', 'm' => 'ₘ', 'o' => 'ₒ', 'r' => 'ᵣ', 't' => 'ₜ', 'x' => 'ₓ', 'p' => 'ₚ', 's' => 'ₛ',
        _ => return None,
    })
}

fn blackboard(c: char) -> Option<char> {
    Some(match c {
        'R' => 'ℝ', 'N' => 'ℕ', 'Z' => 'ℤ', 'Q' => 'ℚ', 'C' => 'ℂ', 'P' => 'ℙ', 'H' => 'ℍ',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::to_text;

    #[test]
    fn symbols_scripts_and_fractions() {
        assert_eq!(to_text(r"E = mc^2"), "E = mc²");
        assert_eq!(to_text(r"x_{i+1} \leq \alpha"), "xᵢ₊₁ ≤ α");
        assert_eq!(to_text(r"\frac{a+b}{2}"), "(a+b)/2");
        assert_eq!(to_text(r"\sqrt{x^2 + y^2}"), "√(x² + y²)");
        assert_eq!(to_text(r"\sum_{k=1}^{n} k"), "∑ₖ₌₁ⁿ k");
        assert_eq!(to_text(r"e^{i\pi}"), "e^(iπ)");
        assert_eq!(to_text(r"\mathbb{R}^3 \to \mathbb{R}"), "ℝ³ → ℝ");
        assert_eq!(to_text(r"\left( \frac{1}{x} \right)"), "( 1/x )");
    }

    #[test]
    fn unknown_and_broken_input_is_kept() {
        assert_eq!(to_text(r"\foo{x}"), r"\foox");
        assert_eq!(to_text(r"\frac{a"), "a/");
        assert_eq!(to_text("x^"), "x^");
        assert_eq!(to_text(r"\"), "");
    }
}
