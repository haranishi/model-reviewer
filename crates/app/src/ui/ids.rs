//! 自動操作が押す部品の名前。

pub const TOOLBAR_OPEN: &str = "toolbar.open";
pub const TOOLBAR_SAVE_REVIEW: &str = "toolbar.save_review";
pub const TOOLBAR_LOAD_REVIEW: &str = "toolbar.load_review";
pub const TOOLBAR_FIT_ALL: &str = "toolbar.fit_all";
pub const TOOLBAR_FIT_SELECTION: &str = "toolbar.fit_selection";
pub const TOOLBAR_HOME: &str = "toolbar.home";
pub const TOOLBAR_ANNOTATE: &str = "toolbar.annotate";
pub const TOOLBAR_HELP: &str = "toolbar.help";
pub const PARTS_SHOW_ALL: &str = "parts.show_all";
pub const PARTS_ROW: &str = "parts.row";
pub const PARTS_EYE: &str = "parts.eye";
pub const PARTS_EXPAND: &str = "parts.expand";
pub const NOTES_SELECT: &str = "notes.select";
pub const NOTES_TEXT: &str = "notes.text";
pub const NOTES_DELETE: &str = "notes.delete";
pub const VIEWS_APPLY: &str = "views.apply";
pub const VIEWS_DELETE: &str = "views.delete";
pub const VIEWPORT: &str = "viewport";
pub const VIEWS_NAME: &str = "views.name";
pub const VIEWS_SAVE: &str = "views.save";
pub const STATUS_WARNINGS: &str = "status.warnings";
pub const STATUS_DETAILS: &str = "status.details";
pub const DIALOG_CONFIRM: &str = "dialog.confirm";
pub const DIALOG_CANCEL: &str = "dialog.cancel";
pub const HELP_CLOSE: &str = "help.close";
pub const HELP_OPEN: &str = "help.open";
pub const DRAFT_TEXT: &str = "draft.text";
pub const DRAFT_COMMIT: &str = "draft.commit";
pub const PANEL_TOOLBAR: &str = "panel.toolbar";
pub const PANEL_PARTS: &str = "panel.parts";
pub const PANEL_NOTES: &str = "panel.notes";
pub const PANEL_STATUS: &str = "panel.status";
pub const PANEL_DETAILS: &str = "panel.details";

// ---- 第2段階 ----
// 道具の列（ui::toolrail）。
pub const TOOL_SELECT: &str = "tool.select";
pub const TOOL_MOVE: &str = "tool.move";
pub const TOOL_ROTATE: &str = "tool.rotate";
pub const TOOL_SCALE: &str = "tool.scale";
pub const TOOL_SHAPE: &str = "tool.shape";
pub const TOOL_MEASURE: &str = "tool.measure";
pub const TOOL_ANNOTATE: &str = "tool.annotate";
pub const SHAPE_CUBE: &str = "shape.cube";
pub const SHAPE_SPHERE: &str = "shape.sphere";
pub const SHAPE_CYLINDER: &str = "shape.cylinder";
pub const SHAPE_PLANE: &str = "shape.plane";
// 3Dビューの上端（ui::viewbar）。
pub const VIEW_DISPLAY_FACES: &str = "view.display.faces";
pub const VIEW_DISPLAY_FACES_LINES: &str = "view.display.faces_lines";
pub const VIEW_DISPLAY_LINES: &str = "view.display.lines";
pub const VIEW_DISPLAY_XRAY: &str = "view.display.xray";
pub const VIEW_PROJECTION_PERSPECTIVE: &str = "view.projection.perspective";
pub const VIEW_PROJECTION_ORTHOGRAPHIC: &str = "view.projection.orthographic";
pub const VIEW_GRID: &str = "view.grid";
// 向きの立方体（ui::viewcube）。見えている面だけ登録する。
pub const VIEWCUBE_FRONT: &str = "viewcube.front";
pub const VIEWCUBE_BACK: &str = "viewcube.back";
pub const VIEWCUBE_LEFT: &str = "viewcube.left";
pub const VIEWCUBE_RIGHT: &str = "viewcube.right";
pub const VIEWCUBE_TOP: &str = "viewcube.top";
pub const VIEWCUBE_BOTTOM: &str = "viewcube.bottom";
// ハンドル（ui::gizmo_view）。`gizmo.<道具>.<軸>`。ポインタが当たりの幅に入っている間だけ出る。
pub const GIZMO_MOVE_X: &str = "gizmo.move.x";
pub const GIZMO_MOVE_Y: &str = "gizmo.move.y";
pub const GIZMO_MOVE_Z: &str = "gizmo.move.z";
pub const GIZMO_ROTATE_X: &str = "gizmo.rotate.x";
pub const GIZMO_ROTATE_Y: &str = "gizmo.rotate.y";
pub const GIZMO_ROTATE_Z: &str = "gizmo.rotate.z";
pub const GIZMO_SCALE_X: &str = "gizmo.scale.x";
pub const GIZMO_SCALE_Y: &str = "gizmo.scale.y";
pub const GIZMO_SCALE_Z: &str = "gizmo.scale.z";
pub const GIZMO_SCALE_ALL: &str = "gizmo.scale.all";
// 数値の欄（ui::inspector）。
pub const INSPECTOR_POS_X: &str = "inspector.pos.x";
pub const INSPECTOR_POS_Y: &str = "inspector.pos.y";
pub const INSPECTOR_POS_Z: &str = "inspector.pos.z";
pub const INSPECTOR_ROT_X: &str = "inspector.rot.x";
pub const INSPECTOR_ROT_Y: &str = "inspector.rot.y";
pub const INSPECTOR_ROT_Z: &str = "inspector.rot.z";
pub const INSPECTOR_SCALE_X: &str = "inspector.scale.x";
pub const INSPECTOR_SCALE_Y: &str = "inspector.scale.y";
pub const INSPECTOR_SCALE_Z: &str = "inspector.scale.z";
// 上端の操作と状態欄。
pub const TOOLBAR_EXPORT: &str = "toolbar.export";
pub const TOOLBAR_UNDO: &str = "toolbar.undo";
pub const TOOLBAR_REDO: &str = "toolbar.redo";
pub const STATUS_UNIT: &str = "status.unit";
