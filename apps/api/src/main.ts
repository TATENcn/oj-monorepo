import { openapi } from "@elysia/openapi";
import { Elysia } from "elysia";
import { authRoutePlugin } from "./modules/auth";

new Elysia()
	.use(authRoutePlugin)
	.use(openapi({ provider: "scalar" }))
	.listen({ port: 3080 });
