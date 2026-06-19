import Elysia from "elysia";
import { initRabbitMq } from "utils";

const res = await initRabbitMq(process.env.RABBIT_MQ_URL!);

export const mqPlugin = new Elysia({ name: "mq" }).decorate("mq", res);
