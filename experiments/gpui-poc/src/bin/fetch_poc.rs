//! 既存の reqwest ベース infrastructure を GPUI から呼べるかの最小検証。
//!
//! - 接続先: 環境変数 `MTR_ENDPOINT`（例: `http://127.0.0.1:49182`）
//! - dimension: 環境変数 `MTR_DIMENSION`（未設定なら 0）
//!
//! reqwest は tokio のリアクタを必要とするため、専用スレッドで current-thread
//! ランタイムを回し、結果を oneshot で GPUI の executor に返す。

use futures::channel::oneshot;
use gpui::{
    App, Application, Bounds, ClickEvent, Context, IntoElement, ParentElement, Render,
    SharedString, Styled, Window, WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use mtr_oudia_application::{MtrApiClient, MtrEndpoint, MtrSnapshotResponse};
use mtr_oudia_infrastructure::ReqwestMtrApiClient;

struct FetchPoc {
    status: SharedString,
    routes: Vec<(String, usize)>,
    loading: bool,
}

impl FetchPoc {
    fn new() -> Self {
        Self {
            status: "「取得」を押してください".into(),
            routes: Vec::new(),
            loading: false,
        }
    }

    fn start_fetch(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }

        let endpoint = match std::env::var("MTR_ENDPOINT") {
            Ok(value) => value,
            Err(_) => {
                self.status =
                    "環境変数 MTR_ENDPOINT を設定してください（例: http://127.0.0.1:49182）".into();
                cx.notify();
                return;
            }
        };
        let dimension = std::env::var("MTR_DIMENSION")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(0);

        self.loading = true;
        self.status = format!("取得中... {endpoint} (dimension={dimension})").into();
        self.routes.clear();
        cx.notify();

        let (tx, rx) = oneshot::channel::<Result<MtrSnapshotResponse, String>>();
        std::thread::spawn(move || {
            let result = (|| -> Result<MtrSnapshotResponse, String> {
                let endpoint = MtrEndpoint::parse(&endpoint).map_err(|err| format!("{err:?}"))?;
                let client = ReqwestMtrApiClient::new().map_err(|err| format!("{err:?}"))?;
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|err| err.to_string())?;
                runtime
                    .block_on(client.fetch_snapshot(&endpoint, dimension))
                    .map_err(|err| format!("{err:?}"))
            })();
            let _ = tx.send(result);
        });

        cx.spawn(async move |this, cx| {
            let result = rx.await;
            this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(Ok(response)) => {
                        this.routes = response
                            .snapshot
                            .routes
                            .iter()
                            .map(|route| (route.display_name.clone(), route.stops.len()))
                            .collect();
                        this.status = format!(
                            "{} 路線 / {} 駅を取得しました",
                            this.routes.len(),
                            response
                                .snapshot
                                .routes
                                .iter()
                                .map(|route| route.stops.len())
                                .sum::<usize>()
                        )
                        .into();
                    }
                    Ok(Err(err)) => {
                        this.status = format!("取得失敗: {err}").into();
                    }
                    Err(_) => {
                        this.status = "取得タスクが終了しました".into();
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl Render for FetchPoc {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let button_label = if self.loading {
            "取得中..."
        } else {
            "取得"
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(gpui::white())
            .text_color(gpui::black())
            .child(div().text_xl().child("GPUI async bridge PoC"))
            .child(div().text_sm().child(self.status.clone()))
            .child(
                div()
                    .id("fetch")
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(gpui::rgb(0x2563eb))
                    .text_color(gpui::white())
                    .cursor_pointer()
                    .child(button_label)
                    .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                        this.start_fetch(cx);
                    })),
            )
            .child(
                div()
                    .id("routes")
                    .flex_1()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(
                        self.routes
                            .iter()
                            .enumerate()
                            .map(|(index, (name, stops))| {
                                div()
                                    .text_sm()
                                    .child(format!("{}. {name} ({stops} 駅)", index + 1))
                            }),
                    ),
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
            |_, cx| cx.new(|_| FetchPoc::new()),
        )
        .unwrap();
        cx.activate(true);
    });
}
