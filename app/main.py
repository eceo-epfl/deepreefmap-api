from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from app.config import config
from app.submissions.views import (
    router as submissions_router,
    job_log_router as submission_job_logs_router,
)
from app.objects.views import router as objects_router
from app.status.views import router as status_router
from app.transects.views import router as transects_router
from app.users.views import router as users_router
from app.root.views import router as root_router

app = FastAPI()

origins = ["*"]

app.add_middleware(
    CORSMiddleware,
    allow_origins=origins,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)


# Routes for Deep Reef Map
app.include_router(
    root_router,
    tags=["root"],
)

app.include_router(
    submissions_router,
    prefix=f"{config.API_PREFIX}/submissions",
    tags=["submissions"],
)
app.include_router(
    objects_router,
    prefix=f"{config.API_PREFIX}/objects",
    tags=["objects"],
)
app.include_router(
    status_router,
    prefix=f"{config.API_PREFIX}/status",
    tags=["status"],
)
app.include_router(
    transects_router,
    prefix=f"{config.API_PREFIX}/transects",
    tags=["transects"],
)
app.include_router(
    users_router,
    prefix=f"{config.API_PREFIX}/users",
    tags=["users"],
)
app.include_router(
    submission_job_logs_router,
    prefix=f"{config.API_PREFIX}/submission_job_logs",
    tags=["submissions", "logs"],
)
