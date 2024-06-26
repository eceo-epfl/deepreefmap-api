"""Add fields and relationship to transect

Revision ID: 98b52878465e
Revises: 2383bd7f9d7a
Create Date: 2024-05-22 14:48:50.232416

"""

from typing import Sequence, Union

from alembic import op
import sqlalchemy as sa
import sqlmodel


# revision identifiers, used by Alembic.
revision: str = "98b52878465e"
down_revision: Union[str, None] = "2383bd7f9d7a"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    # ### commands auto generated by Alembic - please adjust! ###
    op.add_column(
        "inputobject",
        sa.Column("transect_id", sqlmodel.sql.sqltypes.GUID(), nullable=True),
    )
    op.create_foreign_key(
        None, "inputobject", "transect", ["transect_id"], ["id"]
    )
    op.add_column("transect", sa.Column("length", sa.Float(), nullable=True))
    op.add_column("transect", sa.Column("depth", sa.Float(), nullable=True))
    op.add_column(
        "transect",
        sa.Column(
            "created_on",
            sa.DateTime(),
            nullable=False,
            server_default=sa.text("now()"),
        ),
    )
    # ### end Alembic commands ###


def downgrade() -> None:
    # ### commands auto generated by Alembic - please adjust! ###
    op.drop_column("transect", "created_on")
    op.drop_column("transect", "depth")
    op.drop_column("transect", "length")
    op.drop_constraint(None, "inputobject", type_="foreignkey")
    op.drop_column("inputobject", "transect_id")
    # ### end Alembic commands ###
