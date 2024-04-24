"""Add processing fields to object again

Revision ID: 6b0a81457a78
Revises: 02d2ae4ca13d
Create Date: 2024-04-24 10:48:04.111426

"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa
import sqlmodel


# revision identifiers, used by Alembic.
revision: str = "6b0a81457a78"
down_revision: Union[str, None] = "02d2ae4ca13d"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    # ### commands auto generated by Alembic - please adjust! ###
    op.add_column(
        "inputobject",
        sa.Column(
            "processing_has_started",
            sa.Boolean(),
            nullable=False,
            server_default=sa.text("false"),
        ),
    )
    op.add_column(
        "inputobject",
        sa.Column(
            "processing_completed_successfully",
            sa.Boolean(),
            nullable=False,
            server_default=sa.text("false"),
        ),
    )
    op.add_column(
        "inputobject",
        sa.Column(
            "processing_message",
            sqlmodel.sql.sqltypes.AutoString(),
            nullable=True,
        ),
    )
    # ### end Alembic commands ###


def downgrade() -> None:
    # ### commands auto generated by Alembic - please adjust! ###
    op.drop_column("inputobject", "processing_message")
    op.drop_column("inputobject", "processing_completed_successfully")
    op.drop_column("inputobject", "processing_has_started")
    # ### end Alembic commands ###
