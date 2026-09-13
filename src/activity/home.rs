use eframe::egui::Context;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct HomeActivity {
    pub ctx: Context,
}

#[allow(dead_code)]
impl HomeActivity {
    pub fn new(ctx: Context) -> Self {
        Self { ctx }
    }
}
