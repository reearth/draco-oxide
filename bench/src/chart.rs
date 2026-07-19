//! Grouped horizontal bar charts as self-contained SVG files.

/// One data series across all categories. `None` marks a category the series
/// has no measurement for.
pub struct Series<'a> {
    pub name: &'a str,
    pub color: &'a str,
    pub values: Vec<Option<f64>>,
    /// Text shown in place of a bar for `None` values.
    pub na_label: &'a str,
}

const SURFACE: &str = "#fcfcfb";
const TEXT_PRIMARY: &str = "#1a1a19";
const TEXT_SECONDARY: &str = "#5f5e56";
const AXIS: &str = "#d9d8d0";

const WIDTH: f64 = 760.0;
const LABEL_GUTTER: f64 = 178.0;
const VALUE_GUTTER: f64 = 80.0;
const BAR_H: f64 = 14.0;
const BAR_GAP: f64 = 2.0;
const GROUP_GAP: f64 = 14.0;
const TOP: f64 = 64.0;
const BOTTOM: f64 = 16.0;
const FONT: &str = "system-ui, -apple-system, 'Segoe UI', sans-serif";

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A bar anchored at `x0` with a 4px rounded data-end (square at the baseline).
fn bar_path(x0: f64, y: f64, w: f64, h: f64) -> String {
    let r = 4.0_f64.min(w / 2.0);
    format!(
        "M{x0:.1} {y:.1} h{:.1} a{r:.1} {r:.1} 0 0 1 {r:.1} {r:.1} v{:.1} a{r:.1} {r:.1} 0 0 1 -{r:.1} {r:.1} h-{:.1} Z",
        w - r,
        h - 2.0 * r,
        w - r,
    )
}

fn fmt_value(v: f64) -> String {
    if v >= 100.0 {
        format!("{v:.0}")
    } else if v >= 10.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}

/// Renders a grouped horizontal bar chart. Every bar carries a direct value
/// label; the legend names the series.
pub fn grouped_bar_svg(
    title: &str,
    subtitle: &str,
    categories: &[String],
    series: &[Series],
) -> String {
    let plot_w = WIDTH - LABEL_GUTTER - VALUE_GUTTER;
    let group_h = series.len() as f64 * BAR_H + (series.len() as f64 - 1.0) * BAR_GAP;
    let height = TOP + categories.len() as f64 * (group_h + GROUP_GAP) - GROUP_GAP + BOTTOM;

    let max_value = series
        .iter()
        .flat_map(|s| s.values.iter().flatten())
        .fold(0.0_f64, |a, &b| a.max(b))
        .max(f64::EPSILON);

    let mut svg = String::new();
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{WIDTH}\" height=\"{height:.0}\" \
         viewBox=\"0 0 {WIDTH} {height:.0}\" font-family=\"{FONT}\" font-size=\"12\">\n"
    ));
    svg.push_str(&format!(
        "<rect width=\"{WIDTH}\" height=\"{height:.0}\" fill=\"{SURFACE}\"/>\n"
    ));
    svg.push_str(&format!(
        "<text x=\"16\" y=\"24\" font-size=\"15\" font-weight=\"600\" fill=\"{TEXT_PRIMARY}\">{}</text>\n",
        esc(title)
    ));
    svg.push_str(&format!(
        "<text x=\"16\" y=\"42\" fill=\"{TEXT_SECONDARY}\">{}</text>\n",
        esc(subtitle)
    ));

    // Legend, right-aligned in the title row.
    let mut lx = WIDTH
        - 16.0
        - series
            .iter()
            .map(|s| 22.0 + 7.0 * s.name.len() as f64)
            .sum::<f64>();
    for s in series {
        svg.push_str(&format!(
            "<rect x=\"{lx:.1}\" y=\"16\" width=\"10\" height=\"10\" rx=\"2\" fill=\"{}\"/>\n",
            s.color
        ));
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"25\" fill=\"{TEXT_PRIMARY}\">{}</text>\n",
            lx + 14.0,
            esc(s.name)
        ));
        lx += 22.0 + 7.0 * s.name.len() as f64;
    }

    // Baseline.
    svg.push_str(&format!(
        "<line x1=\"{LABEL_GUTTER}\" y1=\"{:.1}\" x2=\"{LABEL_GUTTER}\" y2=\"{:.1}\" stroke=\"{AXIS}\" stroke-width=\"1\"/>\n",
        TOP - 6.0,
        height - BOTTOM + 2.0
    ));

    for (ci, cat) in categories.iter().enumerate() {
        let gy = TOP + ci as f64 * (group_h + GROUP_GAP);
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"end\" fill=\"{TEXT_PRIMARY}\">{}</text>\n",
            LABEL_GUTTER - 8.0,
            gy + group_h / 2.0 + 4.0,
            esc(cat)
        ));
        for (si, s) in series.iter().enumerate() {
            let y = gy + si as f64 * (BAR_H + BAR_GAP);
            match s.values[ci] {
                Some(v) => {
                    let w = (v / max_value * plot_w).max(1.0);
                    svg.push_str(&format!(
                        "<path d=\"{}\" fill=\"{}\"/>\n",
                        bar_path(LABEL_GUTTER, y, w, BAR_H),
                        s.color
                    ));
                    svg.push_str(&format!(
                        "<text x=\"{:.1}\" y=\"{:.1}\" fill=\"{TEXT_SECONDARY}\">{}</text>\n",
                        LABEL_GUTTER + w + 6.0,
                        y + BAR_H - 3.0,
                        fmt_value(v)
                    ));
                }
                None => {
                    svg.push_str(&format!(
                        "<text x=\"{:.1}\" y=\"{:.1}\" fill=\"{TEXT_SECONDARY}\">{}</text>\n",
                        LABEL_GUTTER + 6.0,
                        y + BAR_H - 3.0,
                        esc(s.na_label)
                    ));
                }
            }
        }
    }

    svg.push_str("</svg>\n");
    svg
}
