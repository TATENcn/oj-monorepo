import { connect } from "amqplib";

export const initRabbitMq = async (connectionUrl: string) => {
	const SUBMIT_QUEUE = "submit.queue" as const;
	const RESULT_QUEUE = "result.queue" as const;
	const SUBMIT_ROUTE = "submit" as const;
	const RESULT_ROUTE = "result" as const;
	const EXCHANGE_NAME = "online-judge.exchange" as const;

	const connection = await connect(connectionUrl);

	const channel = await connection.createChannel();
	await channel.assertExchange(EXCHANGE_NAME, "direct", { durable: true });
	await channel.assertQueue(SUBMIT_QUEUE, { durable: true });
	await channel.assertQueue(RESULT_QUEUE, { durable: true });
	await channel.bindQueue(SUBMIT_QUEUE, EXCHANGE_NAME, SUBMIT_ROUTE);
	await channel.bindQueue(RESULT_QUEUE, EXCHANGE_NAME, RESULT_ROUTE);
	await channel.prefetch(1);

	return {
		channel,
		connection,
		config: {
			SUBMIT_QUEUE,
			RESULT_QUEUE,
			SUBMIT_ROUTE,
			RESULT_ROUTE,
			EXCHANGE_NAME,
		},
	};
};
