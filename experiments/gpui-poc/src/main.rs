use gpui::{
    App, Application, Bounds, Context, Window, WindowBounds, WindowOptions, div, prelude::*, px,
    rgb, size,
};

/// GPUI の手触りを確認するための最小アプリ。
/// 状態変更 → `cx.notify()` → 再描画、という基本サイクルだけを試す。
struct GpuiPoc {
    count: i32,
}

impl Render for GpuiPoc {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .size_full()
            .bg(rgb(0x1e1e1e))
            .text_color(rgb(0xffffff))
            .child(div().text_2xl().child("MTR-Link-OuDia GPUI PoC"))
            .child(format!("クリック回数: {}", self.count))
            .child(
                div()
                    .id("increment")
                    .px_4()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(0x2563eb))
                    .cursor_pointer()
                    .child("+1")
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.count += 1;
                        cx.notify();
                    })),
            )
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(640.0), px(480.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| GpuiPoc { count: 0 }),
        )
        .expect("failed to open window");
        cx.activate(true);
    });
}
