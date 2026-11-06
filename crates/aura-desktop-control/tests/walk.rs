//! How a read goes through a window's elements: what it lists and in what order, and what it
//! says when its limits, its time or the program cut it short. The tree here is made up.
use aura_desktop_control::elements::{Expand, Limits, Node, Patterns, Tree, UiError, Walked, walk};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const BUTTON: i32 = 50_000;
const COMBO_BOX: i32 = 50_003;
const LIST_ITEM: i32 = 50_007;
const LIST: i32 = 50_008;
const SCROLL_BAR: i32 = 50_014;
const TEXT: i32 = 50_020;
const TREE_ITEM: i32 = 50_024;
const GROUP: i32 = 50_026;
const WINDOW: i32 = 50_032;
const PANE: i32 = 50_033;

#[derive(Default)]
struct Made {
    nodes: Vec<(Node, Vec<usize>)>,
    failing: HashMap<usize, UiError>,
    opened: Vec<usize>,
}

impl Made {
    fn add(&mut self, parent: Option<usize>, control_type: i32, name: &str) -> usize {
        let index = self.nodes.len();
        let node = Node {
            runtime_id: vec![42, i32::try_from(index).expect("small")],
            control_type,
            name: name.into(),
            enabled: true,
            ..Node::default()
        };
        self.nodes.push((node, Vec::new()));
        if let Some(parent) = parent {
            self.nodes[parent].1.push(index);
        }
        index
    }

    fn node(&mut self, index: usize) -> &mut Node {
        &mut self.nodes[index].0
    }

    fn read(&mut self, limits: &Limits) -> Result<Walked, UiError> {
        walk(self, &0, "测试", limits)
    }
}

impl Tree for Made {
    type Item = usize;

    fn children(&mut self, of: &usize) -> Result<Vec<(usize, Node)>, UiError> {
        self.opened.push(*of);
        if let Some(error) = self.failing.get(of) {
            return Err(error.clone());
        }
        Ok(self.nodes[*of].1.iter().map(|child| (*child, self.nodes[*child].0.clone())).collect())
    }
}

fn limits(max_elements: usize, max_depth: usize) -> Limits {
    Limits { max_elements, max_depth, deadline: Instant::now() + Duration::from_secs(60) }
}

fn outline(walked: &Walked) -> Vec<(u32, &str, bool)> {
    walked
        .elements
        .iter()
        .map(|element| (element.depth, element.key.name.as_str(), element.more))
        .collect()
}

/// A window with a pane, a scroll bar, a list of two and a line of text.
fn window() -> (Made, usize) {
    let mut made = Made::default();
    let root = made.add(None, WINDOW, "测试");
    let pane = made.add(Some(root), PANE, "");
    made.add(Some(pane), BUTTON, "确定");
    made.add(Some(pane), TEXT, "说明");
    let bar = made.add(Some(root), SCROLL_BAR, "");
    made.add(Some(bar), BUTTON, "向上");
    let list = made.add(Some(root), LIST, "水果");
    made.add(Some(list), LIST_ITEM, "苹果");
    made.add(Some(list), LIST_ITEM, "香蕉");
    made.add(Some(root), TEXT, "状态：好");
    (made, list)
}

#[test]
fn a_read_lists_what_can_be_used_and_the_named_parts_in_the_windows_order() {
    let (mut made, _) = window();
    let walked = made.read(&limits(100, 10)).expect("read");
    assert!(walked.complete);
    assert_eq!(
        outline(&walked),
        [
            (0, "确定", false),
            (0, "说明", false),
            (0, "水果", false),
            (1, "苹果", false),
            (1, "香蕉", false),
            (0, "状态：好", false),
        ]
    );
    assert!(!made.opened.contains(&4), "the scroll bar is not opened: {:?}", made.opened);
}

#[test]
fn text_that_repeats_the_name_of_what_it_is_in_is_left_out() {
    let mut made = Made::default();
    let root = made.add(None, WINDOW, "测试");
    let apple = made.add(Some(root), LIST_ITEM, "苹果");
    made.add(Some(apple), TEXT, "苹果");
    made.add(Some(apple), TEXT, "红色");
    let settings = made.add(Some(root), GROUP, "设置");
    made.add(Some(settings), TEXT, "设置");
    made.add(Some(settings), BUTTON, "保存");
    made.add(Some(root), TEXT, "测试");
    let walked = made.read(&limits(100, 10)).expect("read");
    assert_eq!(
        outline(&walked),
        [(0, "苹果", false), (1, "红色", false), (0, "设置", false), (1, "保存", false)]
    );
}

#[test]
fn nothing_off_the_screen_is_listed_or_opened() {
    let mut made = Made::default();
    let root = made.add(None, WINDOW, "测试");
    let hidden = made.add(Some(root), PANE, "藏着的");
    made.node(hidden).offscreen = true;
    made.add(Some(hidden), BUTTON, "里面");
    let below = made.add(Some(root), BUTTON, "下面");
    made.node(below).offscreen = true;
    made.add(Some(root), BUTTON, "看得见");
    let walked = made.read(&limits(100, 10)).expect("read");
    assert_eq!(outline(&walked), [(0, "看得见", false)]);
    assert_eq!(made.opened, [root]);
}

#[test]
fn a_read_at_its_element_limit_marks_what_it_left_out() {
    let (mut made, _) = window();
    let walked = made.read(&limits(4, 10)).expect("read");
    assert!(!walked.complete);
    // Breadth first: the top of the window comes before what is inside the list.
    assert_eq!(
        outline(&walked),
        [(0, "确定", false), (0, "说明", false), (0, "水果", true), (0, "状态：好", false)]
    );
}

#[test]
fn every_part_left_out_at_the_limit_is_marked() {
    let mut made = Made::default();
    let root = made.add(None, WINDOW, "测试");
    let first = made.add(Some(root), LIST, "甲");
    made.add(Some(first), LIST_ITEM, "甲一");
    let second = made.add(Some(root), LIST, "乙");
    made.add(Some(second), LIST_ITEM, "乙一");
    let walked = made.read(&limits(2, 10)).expect("read");
    assert!(!walked.complete);
    assert_eq!(outline(&walked), [(0, "甲", true), (0, "乙", true)]);
    let (mut made, _) = window();
    let walked = made.read(&limits(5, 10)).expect("read");
    assert!(outline(&walked).contains(&(0, "水果", true)), "the last one left out is marked too");
}

#[test]
fn a_read_does_not_open_controls_whose_parts_are_their_own_drawing() {
    let mut made = Made::default();
    let root = made.add(None, WINDOW, "测试");
    let save = made.add(Some(root), BUTTON, "保存");
    made.add(Some(save), TEXT, "Ctrl+S");
    let walked = made.read(&limits(100, 10)).expect("read");
    assert_eq!(outline(&walked), [(0, "保存", false)]);
    assert_eq!(made.opened, [root]);
}

#[test]
fn a_read_marks_what_it_did_not_open_below_its_depth() {
    let (mut made, _) = window();
    let walked = made.read(&limits(100, 1)).expect("read");
    assert!(!walked.complete);
    assert_eq!(outline(&walked), [(0, "水果", true), (0, "状态：好", false)]);
    assert_eq!(made.opened, [0]);
}

#[test]
fn past_its_deadline_a_read_lists_what_it_found_but_opens_nothing_more() {
    let (mut made, _) = window();
    let walked = made
        .read(&Limits { max_elements: 100, max_depth: 10, deadline: Instant::now() })
        .expect("read");
    assert!(!walked.complete);
    assert_eq!(outline(&walked), [(0, "水果", true), (0, "状态：好", false)]);
    assert_eq!(made.opened, [0]);
}

#[test]
fn a_part_that_stops_answering_ends_the_opening_but_keeps_what_was_found() {
    let mut made = Made::default();
    let root = made.add(None, WINDOW, "测试");
    let first = made.add(Some(root), PANE, "甲");
    made.add(Some(first), BUTTON, "一");
    let stuck = made.add(Some(root), LIST, "乙");
    made.add(Some(stuck), LIST_ITEM, "乙一");
    let last = made.add(Some(root), PANE, "丙");
    made.add(Some(last), BUTTON, "二");
    made.failing.insert(stuck, UiError::Timeout);
    let walked = made.read(&limits(100, 10)).expect("read");
    assert!(!walked.complete);
    assert_eq!(
        outline(&walked),
        [(0, "甲", false), (1, "一", false), (0, "乙", true), (0, "丙", true)]
    );
    assert_eq!(made.opened, [root, first, stuck]);
}

#[test]
fn a_part_that_vanished_while_it_was_read_is_passed_over() {
    let (mut made, list) = window();
    made.failing.insert(list, UiError::Gone);
    let walked = made.read(&limits(100, 10)).expect("read");
    assert!(walked.complete);
    assert_eq!(
        outline(&walked),
        [(0, "确定", false), (0, "说明", false), (0, "水果", false), (0, "状态：好", false)]
    );
    made.failing.insert(list, UiError::Failed("坏了".into()));
    let walked = made.read(&limits(100, 10)).expect("read");
    assert!(!walked.complete);
    assert!(outline(&walked).contains(&(0, "水果", true)));
}

#[test]
fn a_read_whose_start_does_not_answer_fails() {
    let (mut made, _) = window();
    made.failing.insert(0, UiError::Timeout);
    assert_eq!(made.read(&limits(100, 10)), Err(UiError::Timeout));
}

#[test]
fn what_holds_its_choices_only_while_expanded_is_opened_only_then() {
    let mut made = Made::default();
    let root = made.add(None, WINDOW, "测试");
    let expandable = Patterns { expand_collapse: true, ..Patterns::default() };
    let closed = made.add(Some(root), COMBO_BOX, "大小");
    made.add(Some(closed), LIST_ITEM, "大");
    let open = made.add(Some(root), TREE_ITEM, "文件夹");
    made.add(Some(open), TREE_ITEM, "子文件夹");
    let shut = made.add(Some(root), TREE_ITEM, "另一个");
    made.add(Some(shut), TREE_ITEM, "藏着");
    for (index, state) in
        [(closed, Expand::Collapsed), (open, Expand::Expanded), (shut, Expand::Collapsed)]
    {
        made.node(index).patterns = expandable;
        made.node(index).expand = Some(state);
    }
    let walked = made.read(&limits(100, 10)).expect("read");
    assert!(walked.complete);
    assert_eq!(
        outline(&walked),
        [(0, "大小", false), (0, "文件夹", false), (1, "子文件夹", false), (0, "另一个", false)]
    );
    assert_eq!(made.opened, [root, open]);
}
