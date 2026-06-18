import { defineRelations } from "drizzle-orm";
import type { BunSQLDatabase } from "drizzle-orm/bun-sql/postgres/driver";
import { char, integer, pgEnum, pgTable, text, timestamp, uuid } from "drizzle-orm/pg-core";
import { user } from "./auth.schema";

export const difficultyEnum = pgEnum("difficulty", ["入门", "普及-", "普及/提高-", "普及+/提高", "提高+/省选-", "省选/NOI-", "NOI/NOI+/CTSC"]);
export const testCaseTypeEnum = pgEnum("test_case_type", ["example", "hidden"]);

export const tags = pgTable("tags", {
	id: uuid("id").primaryKey().defaultRandom(),
	name: text("name").notNull().unique(),
	color: char("color", { length: 8 }).notNull(), // RGBA color
});

export const problems = pgTable("problems", {
	id: uuid("id").primaryKey().defaultRandom(),
	title: text("title").notNull(),
	description: text("description").notNull(),
	difficulty: difficultyEnum("difficulty").notNull(),
	authorId: text("author_id")
		.notNull()
		.references(() => user.id),

	createdAt: timestamp("created_at").notNull().defaultNow(),
	updatedAt: timestamp("updated_at")
		.notNull()
		.defaultNow()
		.$onUpdate(() => new Date()),
	deletedAt: timestamp("deleted_at"),

	limitCpuTimeMs: integer("limit_cpu_time_ms").notNull(),
	limitWallTimeMs: integer("limit_wall_time_ms").notNull(),
	limitMemoryBytes: integer("limit_memory_bytes").notNull(),
	limitOutputBytes: integer("limit_output_bytes").notNull(),
});

export const problemTags = pgTable("problem_tags", {
	problemId: uuid("problem_id")
		.primaryKey()
		.notNull()
		.references(() => problems.id),
	tagId: uuid("tag_id")
		.primaryKey()
		.notNull()
		.references(() => tags.id),
});

export const testCases = pgTable("test_cases", {
	id: uuid("id").primaryKey(),
	problemId: uuid("problem_id")
		.notNull()
		.references(() => problems.id),
	input: text("input").notNull(),
	output: text("output").notNull(),
	type: testCaseTypeEnum("type").notNull().default("hidden"),
});

export const schema = {
	tags,
	problems,
	problemTags,
	testCases,
	user,
};

export const relations = defineRelations({ tags, problems, problemTags, testCases, user }, (r) => ({
	problems: {
		problemTags: r.many.problemTags({
			from: r.problems.id,
			to: r.problemTags.problemId,
		}),
		testCases: r.many.testCases({
			from: r.problems.id,
			to: r.testCases.problemId,
		}),
		author: r.one.user({
			from: r.problems.authorId,
			to: r.user.id,
		}),
	},
	tags: {
		problemTags: r.many.problemTags({
			from: r.tags.id,
			to: r.problemTags.tagId,
		}),
	},
	problemTags: {
		problem: r.one.problems({
			from: r.problemTags.problemId,
			to: r.problems.id,
		}),
		tag: r.one.tags({
			from: r.problemTags.tagId,
			to: r.tags.id,
		}),
	},
	testCases: {
		problem: r.one.problems({
			from: r.testCases.problemId,
			to: r.problems.id,
		}),
	},
	user: {
		problems: r.many.problems({
			from: r.user.id,
			to: r.problems.authorId,
		}),
	},
}));

export type Database = BunSQLDatabase<typeof schema, typeof relations>;

export const seedTags = async (database: Database) => {
	const tagValues: Omit<typeof tags.$inferInsert, "id">[] = [];

	await database.insert(tags).values(tagValues).onConflictDoNothing();
};
