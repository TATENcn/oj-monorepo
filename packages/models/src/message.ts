import type { VerdictResponse, VerdictTask } from "./judge-core";

export type SubmitMessage = { submission_id: string; task: VerdictTask };
export type ResultMessage = { submission_id: string; result: VerdictResponse };

export const AMQP_TOPOLOGY = {
	EXCHANGE_NAME: "online-judge.exchange",
	SUBMIT_QUEUE: "submit.queue",
	RESULT_QUEUE: "result.queue",
	SUBMIT_ROUTE: "submit",
	RESULT_ROUTE: "result",
} as const;
