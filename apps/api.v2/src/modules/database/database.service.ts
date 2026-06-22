import { Injectable } from "@nestjs/common";
import { drizzle } from "drizzle-orm/bun-sql";

@Injectable()
export class DatabaseService {
	private drizzle;

	constructor() {
		this.drizzle = drizzle(process.env.DATABASE_URL!);
	}

	public get db() {
		return this.drizzle;
	}
}
