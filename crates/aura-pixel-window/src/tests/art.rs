use crate::art::{self, Cells};

#[test]
fn the_design_paths_become_whole_cells_inside_their_grid() {
    for picture in
        [&art::LOGO, &art::CLOSE, &art::CHECK, &art::TODO, &art::WARNING, &art::TICK, &art::FISH]
    {
        let mut count = 0;
        for layer in picture.layers {
            for cells in layer.cells() {
                count += 1;
                assert!(cells.width > 0 && cells.height > 0, "{}: {cells:?}", picture.name);
                assert!(cells.x >= 0 && cells.y >= 0, "{}: {cells:?}", picture.name);
                assert!(cells.x + cells.width <= picture.grid.0, "{}: {cells:?}", picture.name);
                assert!(cells.y + cells.height <= picture.grid.1, "{}: {cells:?}", picture.name);
            }
        }
        assert!(count > 0, "{} has cells", picture.name);
    }
}

#[test]
fn runs_with_relative_and_absolute_returns_read_alike() {
    let logo = art::LOGO.layers[0].cells();
    assert_eq!(logo.len(), 66);
    assert_eq!(logo[0], Cells { x: 13, y: 1, width: 6, height: 1 });
    let tick = art::TICK.layers[0].cells();
    assert_eq!(tick.len(), 7);
    assert_eq!(tick[0], Cells { x: 8, y: 3, width: 1, height: 1 });
    assert_eq!(art::TODO.layers[0].cells(), vec![Cells { x: 7, y: 7, width: 2, height: 2 }]);
}

#[test]
fn the_spinner_lead_moves_one_dot_clockwise_each_step() {
    assert_eq!(art::spinner_alpha(0, 0), 1.0);
    assert_eq!(art::spinner_alpha(1, 1), 1.0);
    assert_eq!(art::spinner_alpha(0, 1), art::SPINNER_ALPHA[7]);
    assert_eq!(art::spinner_alpha(7, 8), art::SPINNER_ALPHA[7]);
}
