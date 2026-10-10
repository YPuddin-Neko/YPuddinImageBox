CREATE TABLE fanbox_subscription_state (
    subscription_id INTEGER PRIMARY KEY REFERENCES subscriptions(id) ON DELETE CASCADE,
    state TEXT NOT NULL
);
CREATE INDEX posts_fanbox_resource ON posts (post_url, file_url) WHERE source = 'fanbox';
