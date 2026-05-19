use sea_orm::entity::prelude::*;

pub mod guild_settings {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "guild_settings")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub guild_id: i64,

        pub admin_permission: String,

        pub default_speaker_id: Option<i32>,
        pub default_speed:      Option<f32>,
        pub default_pitch:      Option<f32>,
        pub default_intonation: Option<f32>,

        pub read_embed:           bool,
        pub read_non_vc_user:     bool,
        pub read_server_muted:    bool,
        pub read_username:        bool,
        pub read_spoiler:         bool,
        pub read_only_mentioned:  bool,
        pub read_silent:          bool,

        pub read_vc_join:         bool,
        pub read_vc_leave:        bool,
        pub read_vc_move:         bool,
        pub read_vc_camera_on:    bool,
        pub read_vc_camera_off:   bool,
        pub read_vc_stream_start: bool,
        pub read_vc_stream_stop:  bool,

        pub reply_prefix_type: i32,

        pub music_enabled:      bool,
        pub default_music_vol:  f32,
        pub restrict_music_skip: bool,

        pub command_prefix: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod user_settings {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "user_settings")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub guild_id: i64,
        #[sea_orm(primary_key, auto_increment = false)]
        pub user_id: i64,
        pub speaker_id: Option<i32>,
        pub speed: Option<f32>,
        pub pitch: Option<f32>,
        pub intonation: Option<f32>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
