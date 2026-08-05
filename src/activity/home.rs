use eframe::egui::Context;

#[derive(Debug, Clone)]
pub struct HomeActivity {
    pub ctx: Context,
    
    
}

impl HomeActivity {
    pub fn new(ctx: Context) -> Self {
        Self { ctx }
    }
}