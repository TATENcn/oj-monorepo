import { SQL } from "bun";
import { drizzle } from "drizzle-orm/bun-sql";
import { migrate } from "drizzle-orm/bun-sql/migrator";
import { relations, schema, seedTags } from "./schema";

const client = new SQL({ url: process.env.DATABASE_URL });
const database = drizzle({ client, relations, schema });
await migrate(database, { migrationsFolder: "./drizzle" });
await seedTags(database);

export { database };
