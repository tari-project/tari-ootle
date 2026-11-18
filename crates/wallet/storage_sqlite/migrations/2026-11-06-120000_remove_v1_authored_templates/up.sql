-- Functions stored in the V1 template definition layout carry an `is_mut` key. Templates with a V1 definition do not
-- load, so the wallet drops them from its authored templates.
DELETE FROM authored_templates WHERE functions LIKE '%"is_mut":%';
