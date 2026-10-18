CREATE TABLE `hello` (
	`id` text PRIMARY KEY NOT NULL,
	`name` text NOT NULL,
	`created_at` text NOT NULL,
	`updated_at` text NOT NULL,
	CONSTRAINT "hello_name_length" CHECK(length("hello"."name") BETWEEN 1 AND 80)
) STRICT;
