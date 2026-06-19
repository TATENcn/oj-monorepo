CREATE TYPE "difficulty" AS ENUM('入门', '普及-', '普及/提高-', '普及+/提高', '提高+/省选-', '省选/NOI-', 'NOI/NOI+/CTSC');--> statement-breakpoint
CREATE TYPE "test_case_type" AS ENUM('example', 'hidden');--> statement-breakpoint
CREATE TABLE "problem_tags" (
	"problem_id" uuid PRIMARY KEY,
	"tag_id" uuid NOT NULL
);
--> statement-breakpoint
CREATE TABLE "problems" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
	"title" text NOT NULL,
	"description" text NOT NULL,
	"difficulty" "difficulty" NOT NULL,
	"author_id" text NOT NULL,
	"created_at" timestamp DEFAULT now() NOT NULL,
	"updated_at" timestamp DEFAULT now() NOT NULL,
	"deleted_at" timestamp,
	"limit_cpu_time_ms" integer NOT NULL,
	"limit_wall_time_ms" integer NOT NULL,
	"limit_memory_bytes" integer NOT NULL,
	"limit_output_bytes" integer NOT NULL
);
--> statement-breakpoint
CREATE TABLE "tags" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
	"name" text NOT NULL UNIQUE,
	"color" char(8) NOT NULL
);
--> statement-breakpoint
CREATE TABLE "test_cases" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
	"problem_id" uuid NOT NULL,
	"input" text NOT NULL,
	"output" text NOT NULL,
	"type" "test_case_type" DEFAULT 'hidden'::"test_case_type" NOT NULL
);
--> statement-breakpoint
ALTER TABLE "problem_tags" ADD CONSTRAINT "problem_tags_problem_id_problems_id_fkey" FOREIGN KEY ("problem_id") REFERENCES "problems"("id");--> statement-breakpoint
ALTER TABLE "problem_tags" ADD CONSTRAINT "problem_tags_tag_id_tags_id_fkey" FOREIGN KEY ("tag_id") REFERENCES "tags"("id");--> statement-breakpoint
ALTER TABLE "problems" ADD CONSTRAINT "problems_author_id_user_id_fkey" FOREIGN KEY ("author_id") REFERENCES "user"("id");--> statement-breakpoint
ALTER TABLE "test_cases" ADD CONSTRAINT "test_cases_problem_id_problems_id_fkey" FOREIGN KEY ("problem_id") REFERENCES "problems"("id");