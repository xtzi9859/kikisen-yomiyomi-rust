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
        pub default_speed: Option<f32>,
        pub default_pitch: Option<f32>,
        pub default_intonation: Option<f32>,

        pub read_embed: bool,
        pub read_non_vc_user: bool,
        pub read_server_muted: bool,
        pub read_username: bool,
        pub read_spoiler: bool,
        pub read_only_mentioned: bool,
        pub read_silent: bool,

        pub read_vc_join: bool,
        pub read_vc_leave: bool,
        pub read_vc_move: bool,
        pub read_vc_camera_on: bool,
        pub read_vc_camera_off: bool,
        pub read_vc_stream_start: bool,
        pub read_vc_stream_stop: bool,

        pub reply_prefix_type: i32,

        pub music_enabled: bool,
        pub default_music_vol: f32,
        pub restrict_music_skip: bool,

        pub command_prefix: String,
    }

    impl Model {
        pub fn default_for_guild(guild_id: i64) -> Self {
            Self {
                guild_id,
                admin_permission: "manage_guild".to_string(),
                default_speaker_id: None,
                default_speed: None,
                default_pitch: None,
                default_intonation: None,
                read_embed: false,
                read_non_vc_user: true,
                read_server_muted: false,
                read_username: false,
                read_spoiler: false,
                read_only_mentioned: false,
                read_silent: true,
                read_vc_join: true,
                read_vc_leave: true,
                read_vc_move: true,
                read_vc_camera_on: true,
                read_vc_camera_off: false,
                read_vc_stream_start: true,
                read_vc_stream_stop: false,
                reply_prefix_type: 2,
                music_enabled: true,
                default_music_vol: 0.2,
                restrict_music_skip: false,
                command_prefix: "!".to_string(),
            }
        }
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

pub mod bot_whitelist {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "bot_whitelist")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub guild_id: i64,
        #[sea_orm(primary_key, auto_increment = false)]
        pub bot_id: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod auto_connections {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "auto_connections")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub voice_channel_id: i64,

        pub guild_id: i64,
        pub notify_channel_id: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod reading_targets {
    use super::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "reading_targets")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,

        pub voice_channel_id: i64,
        pub text_channel_id: i64,
        pub guild_id: i64,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}
