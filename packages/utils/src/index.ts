import { connect } from "amqplib";
import { AMQP_TOPOLOGY } from "models/message";

export const initRabbitMq = async (connectionUrl: string) => {
	const connection = await connect(connectionUrl);

	const channel = await connection.createChannel();
	await channel.assertExchange(AMQP_TOPOLOGY.EXCHANGE_NAME, "direct", { durable: true });
	await channel.assertQueue(AMQP_TOPOLOGY.SUBMIT_QUEUE, { durable: true });
	await channel.assertQueue(AMQP_TOPOLOGY.RESULT_QUEUE, { durable: true });
	await channel.bindQueue(AMQP_TOPOLOGY.SUBMIT_QUEUE, AMQP_TOPOLOGY.EXCHANGE_NAME, AMQP_TOPOLOGY.SUBMIT_ROUTE);
	await channel.bindQueue(AMQP_TOPOLOGY.RESULT_QUEUE, AMQP_TOPOLOGY.EXCHANGE_NAME, AMQP_TOPOLOGY.RESULT_ROUTE);
	await channel.prefetch(1);

	return {
		channel,
		connection,
	};
};
