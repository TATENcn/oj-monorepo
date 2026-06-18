import { defineConfig } from "drizzle-kit";

export default defineConfig({
	out: "./drizzle",
	schema: ["./src/modules/db/schema.ts", "./src/modules/db/auth.schema.ts"],
	dialect: "postgresql",
	dbCredentials: {
		url: process.env.DATABASE_URL!,
	},
});
