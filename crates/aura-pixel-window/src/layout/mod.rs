//! Lays a page out at 100 %, the way the design's style sheet places it: the scene to draw, the
//! controls the pointer and keyboard reach, and what screen readers are told about each part.
mod body;
mod controls;
mod frame;

use crate::{
    page::{Face, Id, Page},
    scene::{Rect, Scene, Text},
    theme::Rgb,
};

/// How text measures, at 100 %, for the faces the windows use.
pub trait Measure {
    /// The width of `text` on one line.
    fn width(&self, text: &str, face: Face, size: i32) -> i32;
    /// How far below a line's top its baseline sits, for lines `line` px high.
    fn baseline(&self, face: Face, size: i32, line: i32) -> i32;
    /// How many lines `text` takes wrapped at `width`, when drawn at scale `k`.
    fn lines(&self, text: &str, face: Face, size: i32, width: i32, k: i32) -> i32;
}

/// What the pointer and keyboard are doing: the control under the pointer, the one held down,
/// the one with the focus, whether focus is shown (after the keyboard moved it), and the step of
/// anything that moves by itself.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Look {
    pub hover: Option<usize>,
    pub pressed: Option<usize>,
    pub focus: Option<usize>,
    pub cues: bool,
    pub phase: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    Button(Id),
    Check(Id),
    Link(Id),
    Close,
}

#[derive(Clone, Debug)]
pub struct Control {
    pub target: Target,
    pub rect: Rect,
    pub enabled: bool,
    pub tab_stop: bool,
    pub primary: bool,
    /// Its entry in [`Laid::nodes`].
    pub node: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Heading,
    Text,
    Edit,
    Button,
    CheckBox,
    Hyperlink,
    ProgressBar,
}

/// One part of the window as screen readers see it.
#[derive(Clone, Debug)]
pub struct Node {
    pub role: Role,
    pub name: String,
    pub value: Option<String>,
    /// A step's or mark's state, read after its name.
    pub status: Option<String>,
    pub help: Option<String>,
    pub rect: Rect,
    pub control: Option<usize>,
    /// A progress bar's percentage.
    pub range: Option<f64>,
    pub toggled: Option<bool>,
    pub enabled: bool,
    pub automation_id: String,
}

impl Node {
    fn new(
        role: Role,
        name: impl Into<String>,
        rect: Rect,
        automation_id: impl Into<String>,
    ) -> Self {
        Self {
            role,
            name: name.into(),
            value: None,
            status: None,
            help: None,
            rect,
            control: None,
            range: None,
            toggled: None,
            enabled: true,
            automation_id: automation_id.into(),
        }
    }
}

pub struct Laid {
    pub scene: Scene,
    pub controls: Vec<Control>,
    pub nodes: Vec<Node>,
    /// Where dragging moves the window.
    pub title_bar: Rect,
}

struct Builder<'a> {
    scene: Scene,
    controls: Vec<Control>,
    nodes: Vec<Node>,
    look: Look,
    measure: &'a dyn Measure,
    k: i32,
}

impl Builder<'_> {
    fn node(&mut self, node: Node) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    fn control(
        &mut self,
        target: Target,
        rect: Rect,
        enabled: bool,
        primary: bool,
        node: usize,
    ) -> usize {
        let tab_stop = enabled && target != Target::Close;
        self.controls.push(Control { target, rect, enabled, tab_stop, primary, node });
        let index = self.controls.len() - 1;
        self.nodes[node].control = Some(index);
        self.nodes[node].enabled = enabled;
        index
    }

    /// Under the pointer, held down while under it, focused.
    fn state(&self, index: usize) -> (bool, bool, bool) {
        let hover = self.look.hover == Some(index);
        (hover, hover && self.look.pressed == Some(index), self.look.focus == Some(index))
    }

    fn width(&self, text: &str, face: Face, size: i32) -> i32 {
        self.measure.width(text, face, size)
    }

    /// Words on one line; `width` limits them (they end in an ellipsis) and right-aligns them
    /// when `right` is set. Returns how wide they are drawn.
    #[allow(clippy::too_many_arguments)]
    fn words(
        &mut self,
        text: &str,
        (face, size, line): (Face, i32, i32),
        (colour, background): (Rgb, Rgb),
        x: i32,
        top: i32,
        width: Option<i32>,
        right: bool,
    ) -> i32 {
        let natural = self.width(text, face, size);
        let width = width.unwrap_or(natural);
        let baseline = self.measure.baseline(face, size, line);
        self.scene.text(Text {
            text: text.to_owned(),
            face,
            size,
            colour,
            background,
            x,
            top,
            width,
            line,
            lines: 1,
            baseline,
            wrap: false,
            right_align: right,
        });
        natural.min(width)
    }

    /// Words wrapped at `width`; returns the height they take.
    #[allow(clippy::too_many_arguments)]
    fn wrapped(
        &mut self,
        text: &str,
        (face, size, line): (Face, i32, i32),
        (colour, background): (Rgb, Rgb),
        x: i32,
        top: i32,
        width: i32,
    ) -> i32 {
        let lines = self.measure.lines(text, face, size, width, self.k).max(1);
        let baseline = self.measure.baseline(face, size, line);
        self.scene.text(Text {
            text: text.to_owned(),
            face,
            size,
            colour,
            background,
            x,
            top,
            width,
            line,
            lines,
            baseline,
            wrap: true,
            right_align: false,
        });
        lines * line
    }
}

/// Lays `page` out. `k` is the scale it will be drawn at, which only decides where wrapped
/// words break.
pub fn lay_out(page: &Page, look: &Look, measure: &dyn Measure, k: i32) -> Laid {
    let mut builder = Builder {
        scene: Scene::default(),
        controls: Vec::new(),
        nodes: Vec::new(),
        look: *look,
        measure,
        k: k.max(1),
    };
    let width = frame::window_width(&mut builder, page);
    let column = frame::column_width(&page.frame);
    let content = Rect::new(1 + column, frame::MAIN_TOP, width - 2 - column, 0);
    frame::mark_nodes(&mut builder, page);
    // The body comes first, so the column and footer know how tall the window is; its items are
    // drawn after the background they sit on.
    let body_height = body::lay_out(&mut builder, page, content);
    let body_items = std::mem::take(&mut builder.scene.items);
    let footer_height = controls::footer_height(page);
    let main = (body_height + footer_height).max(frame::min_main(&page.frame));
    let height = frame::MAIN_TOP + main + 1;
    builder.scene.width = width;
    builder.scene.height = height;
    frame::shell(&mut builder, page, width, height);
    frame::column(&mut builder, page, Rect::new(1, frame::MAIN_TOP, column, main));
    builder.scene.items.extend(body_items);
    controls::footer(
        &mut builder,
        page,
        Rect::new(content.x, frame::MAIN_TOP + main - footer_height, content.width, footer_height),
    );
    frame::close_button(&mut builder, page, width);
    Laid {
        scene: builder.scene,
        controls: builder.controls,
        nodes: builder.nodes,
        title_bar: Rect::new(1, 1, width - 2, frame::BAR_HEIGHT),
    }
}
