import { eq } from "drizzle-orm";
import Elysia, { t } from "elysia";
import type { VerdictTask } from "models/judge-core";
import type { ResultMessage, SubmitMessage } from "models/message";
import { AMQP_TOPOLOGY } from "models/message";
import { authPlugin } from "../auth";
import { databasePlugin } from "../db";
import { acceptableLanguageEnumLiteral } from "../db/enums";
import { problems, submissions, testCases } from "../db/schema";
import { mqPlugin } from "./amqp";

const acceptableLanguageEnum = t.Enum(Object.fromEntries(acceptableLanguageEnumLiteral.map((v) => [v, v])));

export const submissionPlugin = new Elysia({ name: "submission" })
	.use(databasePlugin)
	.use(mqPlugin)
	.use(authPlugin)
	.onStart(async (app) => {
		const { channel } = app.decorator.mq;
		const db = app.decorator.db;
		await channel.consume(
			AMQP_TOPOLOGY.RESULT_QUEUE,
			async (msg) => {
				if (!msg) return;
				try {
					const { submission_id, result }: ResultMessage = JSON.parse(msg.content.toString());
					await db.update(submissions).set({ status: "completed", result, completedAt: new Date() }).where(eq(submissions.id, submission_id));
					channel.ack(msg);
				} catch (err) {
					console.error("failed to process verdict result:", err);
					channel.nack(msg);
				}
			},
			{ noAck: false },
		);
	})
	.post(
		"/",
		async ({ db, user, mq, body, status }) => {
			const [problem] = await db
				.select({
					limitCpuTimeMs: problems.limitCpuTimeMs,
					limitWallTimeMs: problems.limitWallTimeMs,
					limitMemoryBytes: problems.limitMemoryBytes,
					limitOutputBytes: problems.limitOutputBytes,
				})
				.from(problems)
				.where(eq(problems.id, body.problemId))
				.limit(1);
			if (!problem) return status(404, undefined);

			const cases = await db.select({ input: testCases.input, output: testCases.output }).from(testCases).where(eq(testCases.problemId, body.problemId));

			const [submission] = await db
				.insert(submissions)
				.values({
					problemId: body.problemId,
					userId: user.id,
					sourceCode: body.sourceCode,
					language: body.language,
				})
				.returning({ id: submissions.id });
			if (!submission) throw new Error("failed to insert submission");

			const task: VerdictTask = {
				source: body.sourceCode,
				language: body.language,
				cases: cases.map((tc) => ({ input: tc.input, output: tc.output })),
				limits: {
					cpu_time_ms: problem.limitCpuTimeMs,
					wall_time_ms: problem.limitWallTimeMs,
					memory_bytes: problem.limitMemoryBytes,
					output_bytes: problem.limitOutputBytes,
				},
			};

			const msg: SubmitMessage = { submission_id: submission.id, task };
			mq.channel.publish(AMQP_TOPOLOGY.EXCHANGE_NAME, AMQP_TOPOLOGY.SUBMIT_ROUTE, Buffer.from(JSON.stringify(msg)));

			return status(202, { id: submission.id });
		},
		{
			auth: true,
			body: t.Object({
				sourceCode: t.String(),
				problemId: t.String({ format: "uuid" }),
				language: acceptableLanguageEnum,
			}),
			response: {
				202: t.Object({ id: t.String({ format: "uuid" }) }),
				404: t.Undefined(),
			},
			detail: { description: "Submit a solution for judging", tags: ["Submissions"] },
		},
	)
	.get(
		"/:id",
		async ({ db, params: { id }, status }) => {
			const [submission] = await db
				.select({ id: submissions.id, status: submissions.status, result: submissions.result })
				.from(submissions)
				.where(eq(submissions.id, id))
				.limit(1);
			if (!submission) return status(404, undefined);
			if (submission.status === "pending") return status(202, undefined);
			return submission.result;
		},
		{
			auth: true,
			params: t.Object({ id: t.String({ format: "uuid" }) }),
			response: {
				202: t.Undefined(),
				404: t.Undefined(),
			},
			detail: { description: "Get submission verdict", tags: ["Submissions"] },
		},
	);
