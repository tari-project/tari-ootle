-- The public keys of every authorizer of the request's transaction, a JSON `AuthorizerSet`. Every authorization signs
-- over the whole set, so it is fixed when the request is created and `message_hash` commits to it.
ALTER TABLE signing_requests ADD COLUMN authorizers TEXT NOT NULL DEFAULT '[]';
