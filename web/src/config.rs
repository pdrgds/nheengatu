pub struct Config {
    pub port: u16,
    pub app_base_url: String,
    pub supabase_url: String,
    pub supabase_anon_key: String,
    pub supabase_service_role_key: String,
    pub supabase_jwt_secret: String,
    pub stripe_secret_key: String,
    pub stripe_webhook_secret: String,
    pub groq_api_key: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            port: std::env::var("PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(3000),
            app_base_url: std::env::var("APP_BASE_URL").expect("APP_BASE_URL must be set"),
            supabase_url: std::env::var("SUPABASE_URL").expect("SUPABASE_URL must be set"),
            supabase_anon_key: std::env::var("SUPABASE_ANON_KEY").expect("SUPABASE_ANON_KEY must be set"),
            supabase_service_role_key: std::env::var("SUPABASE_SERVICE_ROLE_KEY")
                .expect("SUPABASE_SERVICE_ROLE_KEY must be set"),
            supabase_jwt_secret: std::env::var("SUPABASE_JWT_SECRET")
                .expect("SUPABASE_JWT_SECRET must be set"),
            stripe_secret_key: std::env::var("STRIPE_SECRET_KEY")
                .expect("STRIPE_SECRET_KEY must be set"),
            stripe_webhook_secret: std::env::var("STRIPE_WEBHOOK_SECRET")
                .expect("STRIPE_WEBHOOK_SECRET must be set"),
            groq_api_key: std::env::var("GROQ_API_KEY").expect("GROQ_API_KEY must be set"),
        }
    }
}
