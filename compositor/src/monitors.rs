//! Reads the display layout the user saved in GNOME (`~/.config/monitors.xml`), so that
//! this desktop drives each monitor in a mode already known to work on it, arranged the
//! way the monitors physically stand.

use std::path::PathBuf;

/// One monitor's saved settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    /// The connector, as GNOME names it (see [`gnome_name`]).
    pub connector: String,
    pub width: i32,
    pub height: i32,
    /// Refresh rate in Hz.
    pub rate: f64,
    pub x: i32,
    pub y: i32,
    pub scale: f64,
    pub primary: bool,
}

/// The layout to use for a set of connected monitors.
#[derive(Debug, Default, PartialEq)]
pub struct Layout {
    pub monitors: Vec<Saved>,
    /// Whether the positions describe exactly the connected set, and so can be used.
    /// Otherwise only the modes are meaningful.
    pub arranged: bool,
}

impl Layout {
    pub fn get(&self, connector: &str) -> Option<&Saved> {
        self.monitors.iter().find(|saved| saved.connector == connector)
    }
}

/// The name GNOME gives a connector the kernel calls `kernel`: the same, except that
/// `HDMI-A-1` is `HDMI-1`.
pub fn gnome_name(kernel: &str) -> String {
    kernel.replacen("HDMI-A-", "HDMI-", 1)
}

/// The text between `<name>` and `</name>`, for each occurrence in `text`.
fn blocks<'a>(text: &'a str, name: &str) -> Vec<&'a str> {
    let (open, close) = (format!("<{name}>"), format!("</{name}>"));
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(&open) {
        let body = &rest[start + open.len()..];
        let Some(end) = body.find(&close) else {
            break;
        };
        found.push(&body[..end]);
        rest = &body[end + close.len()..];
    }
    found
}

fn value<T: std::str::FromStr>(text: &str, name: &str) -> Option<T> {
    blocks(text, name).first()?.trim().parse().ok()
}

/// Every saved configuration, each a list of the monitors it turns on.
pub fn parse(text: &str) -> Vec<Vec<Saved>> {
    blocks(text, "configuration")
        .into_iter()
        .map(|configuration| {
            blocks(configuration, "logicalmonitor")
                .into_iter()
                .filter_map(|logical| {
                    let mode = blocks(logical, "mode").first().copied()?;
                    Some(Saved {
                        connector: value(logical, "connector")?,
                        width: value(mode, "width")?,
                        height: value(mode, "height")?,
                        rate: value(mode, "rate")?,
                        x: value(logical, "x")?,
                        y: value(logical, "y")?,
                        scale: value(logical, "scale").unwrap_or(1.0),
                        primary: value::<String>(logical, "primary").is_some_and(|primary| primary == "yes"),
                    })
                })
                .collect()
        })
        .collect()
}

/// Chooses what applies to the `connected` monitors (GNOME names): the configuration
/// saved for exactly that set, or failing that, each monitor's mode from wherever it
/// appears.
pub fn pick(configurations: &[Vec<Saved>], connected: &[String]) -> Layout {
    let exact = configurations.iter().find(|configuration| {
        configuration.len() == connected.len()
            && connected.iter().all(|name| configuration.iter().any(|saved| &saved.connector == name))
    });
    if let Some(configuration) = exact {
        return Layout {
            monitors: configuration.clone(),
            // Positions are in scaled coordinates; this desktop does not scale.
            arranged: configuration.iter().all(|saved| saved.scale == 1.0),
        };
    }
    Layout {
        monitors: connected
            .iter()
            .filter_map(|name| configurations.iter().flatten().find(|saved| &saved.connector == name))
            .cloned()
            .collect(),
        arranged: false,
    }
}

/// Reads layouts saved by this desktop's own display settings: blocks separated by
/// blank lines, one monitor per line as `<connector> <width>x<height>@<Hz> <x>,<y>`,
/// with ` main` after the main display.
pub fn parse_own(text: &str) -> Vec<Vec<Saved>> {
    let mut configurations = Vec::new();
    let mut block: Vec<Saved> = Vec::new();
    for line in text.lines().chain([""]) {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            if !block.is_empty() {
                configurations.push(std::mem::take(&mut block));
            }
            continue;
        }
        let parsed = (|| {
            let mut words = line.split_whitespace();
            let connector = words.next()?.to_string();
            let (size, rate) = words.next()?.split_once('@')?;
            let (width, height) = size.split_once('x')?;
            let (x, y) = words.next()?.split_once(',')?;
            Some(Saved {
                connector,
                width: width.parse().ok()?,
                height: height.parse().ok()?,
                rate: rate.parse().ok()?,
                x: x.parse().ok()?,
                y: y.parse().ok()?,
                scale: 1.0,
                primary: words.next() == Some("main"),
            })
        })();
        // A line that cannot be read spoils its whole layout, not the file.
        match parsed {
            Some(saved) => block.push(saved),
            None => {
                block.clear();
                block.push(Saved {
                    connector: String::new(),
                    width: 0,
                    height: 0,
                    rate: 0.0,
                    x: 0,
                    y: 0,
                    scale: 0.0,
                    primary: false,
                });
            }
        }
    }
    configurations.retain(|block| block.iter().all(|saved| !saved.connector.is_empty()));
    configurations
}

pub fn format_own(configurations: &[Vec<Saved>]) -> String {
    let mut text = String::from("# Display layouts chosen in the edex-rs settings. One block per set of monitors.\n");
    for block in configurations {
        text.push('\n');
        for saved in block {
            let main = if saved.primary { " main" } else { "" };
            text.push_str(&format!(
                "{} {}x{}@{:.3} {},{}{main}\n",
                saved.connector, saved.width, saved.height, saved.rate, saved.x, saved.y
            ));
        }
    }
    text
}

fn own_path() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .map(|dir| dir.join("edex-rs/displays"))
}

/// Whether two layouts are for the same set of monitors.
fn same_set(a: &[Saved], b: &[Saved]) -> bool {
    a.len() == b.len() && a.iter().all(|saved| b.iter().any(|other| other.connector == saved.connector))
}

/// Puts `layout` among `configurations`, in place of any for the same monitors.
pub fn replace(configurations: &mut Vec<Vec<Saved>>, layout: &[Saved]) {
    configurations.retain(|block| !same_set(block, layout));
    configurations.push(layout.to_vec());
}

/// Saves a layout chosen in the settings, for the next time these monitors are connected.
pub fn save_own(layout: &[Saved]) -> std::io::Result<()> {
    let path = own_path().ok_or_else(|| std::io::Error::other("no home directory"))?;
    let mut configurations = std::fs::read_to_string(&path).map(|text| parse_own(&text)).unwrap_or_default();
    replace(&mut configurations, layout);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, format_own(&configurations))
}

/// Every saved layout: this desktop's own first, so they win, then GNOME's.
pub fn load_all() -> Vec<Vec<Saved>> {
    let mut configurations = own_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| parse_own(&text))
        .unwrap_or_default();
    configurations.extend(load());
    configurations
}

/// Reads GNOME's saved configurations; none if there is no such file.
fn load() -> Vec<Vec<Saved>> {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    config_home
        .and_then(|dir| std::fs::read_to_string(dir.join("monitors.xml")).ok())
        .map(|text| parse(&text))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<monitors version="2">
  <configuration>
    <layoutmode>logical</layoutmode>
    <logicalmonitor>
      <x>1920</x><y>0</y><scale>1</scale>
      <monitor>
        <monitorspec><connector>HDMI-1</connector><vendor>TOL</vendor></monitorspec>
        <mode><width>1920</width><height>1080</height><rate>60.000</rate></mode>
      </monitor>
    </logicalmonitor>
    <logicalmonitor>
      <x>0</x><y>0</y><scale>1</scale><primary>yes</primary>
      <monitor>
        <monitorspec><connector>eDP-1</connector></monitorspec>
        <mode><width>1920</width><height>1080</height><rate>60.052</rate></mode>
      </monitor>
    </logicalmonitor>
  </configuration>
  <configuration>
    <logicalmonitor>
      <x>0</x><y>0</y><scale>2</scale><primary>yes</primary>
      <monitor>
        <monitorspec><connector>eDP-1</connector></monitorspec>
        <mode><width>1920</width><height>1080</height><rate>60.052</rate></mode>
      </monitor>
    </logicalmonitor>
    <disabled><monitorspec><connector>DP-2</connector></monitorspec></disabled>
  </configuration>
</monitors>"#;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn parses_configurations() {
        let configurations = parse(SAMPLE);
        assert_eq!(configurations.len(), 2);
        assert_eq!(
            configurations[0][0],
            Saved {
                connector: "HDMI-1".into(),
                width: 1920,
                height: 1080,
                rate: 60.0,
                x: 1920,
                y: 0,
                scale: 1.0,
                primary: false,
            }
        );
        assert!(configurations[0][1].primary);
        assert_eq!(configurations[1].len(), 1);
        assert!(parse("").is_empty());
        assert!(parse("<configuration><logicalmonitor><x>oops").is_empty());
    }

    #[test]
    fn picks_the_configuration_for_the_connected_set() {
        let configurations = parse(SAMPLE);
        let both = pick(&configurations, &names(&["eDP-1", "HDMI-1"]));
        assert!(both.arranged);
        assert_eq!(both.get("HDMI-1").unwrap().x, 1920);

        // Saved with scaling: the mode is usable, the positions are not.
        let alone = pick(&configurations, &names(&["eDP-1"]));
        assert!(!alone.arranged);
        assert_eq!(alone.monitors.len(), 1);

        // No configuration for this set: modes only, for the monitors that are known.
        let other = pick(&configurations, &names(&["HDMI-1", "DP-5"]));
        assert!(!other.arranged);
        assert_eq!(other.monitors.len(), 1);
        assert_eq!(other.get("HDMI-1").unwrap().width, 1920);
        assert_eq!(pick(&[], &names(&["eDP-1"])), Layout::default());
    }

    #[test]
    fn own_layouts_round_trip() {
        let layout = vec![
            Saved { connector: "HDMI-1".into(), width: 1920, height: 1080, rate: 60.0, x: 0, y: 0, scale: 1.0, primary: true },
            Saved { connector: "eDP-1".into(), width: 1920, height: 1080, rate: 60.052, x: 1920, y: 0, scale: 1.0, primary: false },
        ];
        let alone = vec![Saved { connector: "eDP-1".into(), width: 1280, height: 720, rate: 60.0, x: 0, y: 0, scale: 1.0, primary: true }];
        let text = format_own(&[layout.clone(), alone.clone()]);
        assert_eq!(parse_own(&text), vec![layout.clone(), alone.clone()]);

        // A newer layout for the same monitors takes the older one's place.
        let mut configurations = vec![layout.clone(), alone.clone()];
        let mut changed = layout.clone();
        changed[0].width = 3840;
        replace(&mut configurations, &changed);
        assert_eq!(configurations, vec![alone, changed.clone()]);
        // And wins over GNOME's when both know the monitors.
        configurations.extend(parse(SAMPLE));
        assert_eq!(pick(&configurations, &names(&["eDP-1", "HDMI-1"])).get("HDMI-1").unwrap().width, 3840);
    }

    #[test]
    fn a_bad_line_spoils_only_its_own_layout() {
        let text = "eDP-1 1920x1080@60 0,0 main\n\nHDMI-1 huge 0,0\neDP-1 1920x1080@60 0,0\n\nDP-1 800x600@75.5 0,0 main\n";
        let configurations = parse_own(text);
        assert_eq!(configurations.len(), 2);
        assert_eq!(configurations[1][0].connector, "DP-1");
        assert_eq!(configurations[1][0].rate, 75.5);
        assert!(parse_own("").is_empty());
    }

    #[test]
    fn translates_connector_names() {
        assert_eq!(gnome_name("HDMI-A-1"), "HDMI-1");
        assert_eq!(gnome_name("eDP-1"), "eDP-1");
        assert_eq!(gnome_name("DP-2"), "DP-2");
    }
}
