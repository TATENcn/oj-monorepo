import { Body, Controller, Delete, Get, HttpCode, HttpStatus, Param, Patch, Post, Query } from "@nestjs/common";
import { ApiAcceptedResponse, ApiCreatedResponse, ApiNoContentResponse, ApiOkResponse, ApiResponse } from "@nestjs/swagger";
import type { User as AuthUser } from "better-auth";
import { Auth, User } from "../auth/session.decorator";
// biome-ignore lint/style/useImportType: Injection
import {
	ApproveContestRequest,
	ContestResponse,
	CreateContestRequest,
	CreateContestResponse,
	DeleteContestRequest,
	GetContestListParams,
	GetContestParams,
	UpdateContestFieldsParams,
	UpdateContestFieldsRequest,
} from "./contest.dto";
// biome-ignore lint/style/useImportType: Injection
import { ContestRepository } from "./contest.repository";

@Controller("api/contests")
export class ContestController {
	constructor(private readonly repository: ContestRepository) {}

	@Post()
	@Auth()
	@HttpCode(HttpStatus.CREATED)
	@ApiResponse({ status: 201, description: "Contest created", type: CreateContestResponse })
	public async createContest(@Body() body: CreateContestRequest, @User() user: AuthUser): Promise<CreateContestResponse> {
		const id = await this.repository.createContest(body, user.id);
		return { id };
	}

	@Get()
	@ApiOkResponse({ description: "List of contests", type: ContestResponse, isArray: true })
	public async getContests(@Query() query: GetContestListParams): Promise<ContestResponse[]> {
		return this.repository.getContests(query);
	}

	@Get(":id")
	@ApiOkResponse({ description: "Contest details", type: ContestResponse })
	public async getContest(@Param() params: GetContestParams): Promise<ContestResponse | null> {
		return this.repository.getContest(params);
	}

	@Patch(":id")
	@Auth()
	@HttpCode(HttpStatus.ACCEPTED)
	public async updateContestFields(@Param() params: UpdateContestFieldsParams, @Body() body: UpdateContestFieldsRequest, @User() user: AuthUser) {
		await this.repository.updateContestFields(body, params, user.id);
	}

	@Delete(":id")
	@Auth()
	@HttpCode(HttpStatus.NO_CONTENT)
	public async deleteContest(@Param() params: DeleteContestRequest, @User() user: AuthUser) {
		await this.repository.deleteContest(params, user.id);
	}

	@Post(":id/approve")
	@Auth()
	@HttpCode(HttpStatus.NO_CONTENT)
	public async approveContest(@Param() params: ApproveContestRequest, @User() user: AuthUser) {
		await this.repository.approveContest(params, user.id);
	}
}
