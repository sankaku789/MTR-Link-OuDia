// GPUI 標準のテキスト入力は存在しないため、公式 example `input.rs` を
// そのまま取り込んで IME / 選択 / クリップボードの挙動を評価する。
mod text_input;

fn main() {
    text_input::run();
}
