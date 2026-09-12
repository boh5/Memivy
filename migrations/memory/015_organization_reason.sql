-- System explanations have stable codes; existing text remains untouched.
ALTER TABLE organization_jobs ADD COLUMN reason_code TEXT;
PRAGMA user_version = 15;
